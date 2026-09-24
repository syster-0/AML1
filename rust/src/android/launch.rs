//! Main-process launch adapter for Android.
//!
//! The game cannot be an external process on Android: there is no `java`
//! binary on the device, and SELinux denies executing files from the data
//! partition. Instead the launcher writes a private launch manifest; the
//! `:game` process ([`GameActivity`]) reads it and boots an in-process
//! HotSpot VM from the staged JRE.
//!
//! Log lines and lifecycle events return from the game over a named
//! Unix-domain socket whose path is embedded in the manifest. Both the
//! socket and the manifest live inside the app-private files area, so no
//! other app can reach them.
//!
//! [`GameActivity`]: crate::android::bridge

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};

use crate::launcher::args::LaunchArgs;
use crate::launcher::install::PreparedLaunch;
use crate::launcher::process::{
    emit_process, ProcessEvent, ProcessMetadata, PROCESS_MANAGER,
};

const SCHEMA_VERSION: u32 = 1;
/// Time the launcher waits for the game to connect after the manifest is
/// written (the activity is started by Dart right after this call returns).
const GAME_CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
}
const SIGKILL: i32 = 9;

/// Launch manifest consumed by the `:game` runtime. The layout is shared
/// between launcher and game; bump [SCHEMA_VERSION] on incompatible changes.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct LaunchManifest {
    pub schema_version: u32,
    /// JRE home under the app-private files area (holds `lib/server/libjvm.so`
    /// and `lib/modules`).
    pub jre_home: String,
    /// Process working directory (the instance directory).
    pub working_dir: String,
    /// Full JVM argument list, including `-cp`.
    pub jvm_args: Vec<String>,
    /// Main class in JNI internal form, e.g. `net/minecraft/client/main/Main`.
    pub main_class: String,
    /// Arguments passed to the main class `main(String[])`.
    pub game_args: Vec<String>,
    /// Path of the launcher's UDS listener for log/lifecycle frames.
    pub ipc_socket: String,
}

/// NDJSON frame sent from the `:game` process, one object per line.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum GameFrame {
    /// First frame after connecting; carries the game pid for kill support.
    Hello { pid: u32 },
    /// One stdout/stderr line from the game.
    Log { line: String },
    /// Game main returned / JVM exited.
    Exit { code: i32 },
}

struct RemoteGame {
    uuid: String,
    instance_id: String,
    pid: RwLock<Option<u32>>,
}

/// In-process game sessions, keyed by launch uuid.
static REMOTE_GAMES: Lazy<RwLock<HashMap<String, RemoteGame>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

/// What the launcher hands back after staging a game launch.
pub(crate) struct GameHandle {
    pub meta: ProcessMetadata,
    pub manifest_path: PathBuf,
}

/// Write the launch manifest, bind the IPC socket and spawn its handler.
/// The caller (Dart) starts `GameActivity` with the returned manifest path.
pub(crate) async fn launch_game(
    prepared: PreparedLaunch,
    files_root: &Path,
) -> Result<GameHandle> {
    if PROCESS_MANAGER
        .get_by_instance(&prepared.instance_id)
        .is_some()
        || REMOTE_GAMES
            .read()
            .unwrap()
            .values()
            .any(|g| g.instance_id == prepared.instance_id)
    {
        return Err(anyhow!("instance already running"));
    }

    let runtime_dir = files_root.join("runtime");
    let manifest_dir = runtime_dir.join("manifests");
    let ipc_dir = runtime_dir.join("ipc");
    tokio::fs::create_dir_all(&manifest_dir).await?;
    tokio::fs::create_dir_all(&ipc_dir).await?;

    // A short id keeps the socket path well under UNIX_PATH_MAX (108 bytes).
    let id = uuid::Uuid::new_v4().simple().to_string();
    let id: String = id.chars().take(12).collect();
    let manifest_path = manifest_dir.join(format!("{id}.json"));
    let socket_path = ipc_dir.join(format!("{id}.sock"));
    let _ = tokio::fs::remove_file(&socket_path).await;

    let listener = UnixListener::bind(&socket_path)
        .with_context(|| format!("bind IPC socket {}", socket_path.display()))?;

    let manifest = build_manifest(prepared.args, files_root, &socket_path)?;

    {
        let json = serde_json::to_vec_pretty(&manifest)?;
        let mut file = tokio::fs::File::create(&manifest_path)
            .await
            .with_context(|| format!("create {}", manifest_path.display()))?;
        file.write_all(&json).await?;
        file.sync_all().await?;
    }

    let uuid = uuid::Uuid::new_v4().to_string();
    REMOTE_GAMES.write().unwrap().insert(
        uuid.clone(),
        RemoteGame {
            uuid: uuid.clone(),
            instance_id: prepared.instance_id.clone(),
            pid: RwLock::new(None),
        },
    );
    emit_process(ProcessEvent {
        instance_id: prepared.instance_id.clone(),
        uuid: uuid.clone(),
        event: "launched".into(),
        message: "Launched Minecraft".into(),
    });

    {
        let uuid = uuid.clone();
        let instance_id = prepared.instance_id.clone();
        let log_path = prepared.log_path.clone();
        let socket_path = socket_path.clone();
        tokio::spawn(async move {
            let accepted =
                tokio::time::timeout(GAME_CONNECT_TIMEOUT, listener.accept()).await;
            match accepted {
                Ok(Ok((stream, _))) => {
                    serve_game(stream, uuid, instance_id, log_path, socket_path).await
                }
                Ok(Err(error)) => {
                    announce_failed_connect(
                        &uuid,
                        &instance_id,
                        format!("IPC accept failed: {error}"),
                        &socket_path,
                    );
                }
                Err(_) => {
                    announce_failed_connect(
                        &uuid,
                        &instance_id,
                        "game did not connect to the launcher IPC socket".into(),
                        &socket_path,
                    );
                }
            }
        });
    }

    Ok(GameHandle {
        meta: ProcessMetadata {
            uuid,
            instance_id: prepared.instance_id,
        },
        manifest_path,
    })
}

/// Convert the shared [LaunchArgs] into the game-side manifest.
///
/// Desktop launches go through the theseus `LauncherEntry` + RPC handshake;
/// the in-process game has no such gate, so those pieces are stripped here
/// and the real Minecraft main class is invoked directly.
fn build_manifest(
    args: LaunchArgs,
    files_root: &Path,
    socket_path: &Path,
) -> Result<LaunchManifest> {
    // `-cp`/`-classpath` are launcher (jli) options; JNI_CreateJavaVM only
    // understands `-Djava.class.path=...`, so merge the marker and value.
    let mut raw = args.jvm_args.into_iter();
    let mut jvm_args: Vec<String> = Vec::new();
    while let Some(arg) = raw.next() {
        if arg == "-cp" || arg == "-classpath" {
            let classpath = raw
                .next()
                .ok_or_else(|| anyhow!("{arg} is missing its classpath value"))?;
            jvm_args.push(format!("-Djava.class.path={classpath}"));
        } else if !arg.starts_with("-javaagent:") && !arg.starts_with("-Dmodrinth.internal.") {
            jvm_args.push(arg);
        }
    }
    if !jvm_args
        .iter()
        .any(|arg| arg.starts_with("-Djava.class.path="))
    {
        return Err(anyhow!("assembled JVM args are missing the classpath"));
    }

    Ok(LaunchManifest {
        schema_version: SCHEMA_VERSION,
        jre_home: files_root.join("jre").to_string_lossy().into_owned(),
        working_dir: args.cwd.to_string_lossy().into_owned(),
        jvm_args,
        main_class: args.minecraft_main_class.replace('.', "/"),
        game_args: args.game_args,
        ipc_socket: socket_path.to_string_lossy().into_owned(),
    })
}

/// Read frames until the game exits or the socket closes.
async fn serve_game(
    stream: UnixStream,
    uuid: String,
    instance_id: String,
    log_path: PathBuf,
    socket_path: PathBuf,
) {
    if let Some(parent) = log_path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    let mut lines = BufReader::new(stream).lines();
    let mut exit_code: Option<i32> = None;
    while let Ok(Some(text)) = lines.next_line().await {
        match serde_json::from_str::<GameFrame>(&text) {
            Ok(GameFrame::Hello { pid }) => {
                if let Some(game) = REMOTE_GAMES.read().unwrap().get(&uuid) {
                    *game.pid.write().unwrap() = Some(pid);
                }
            }
            Ok(GameFrame::Log { line }) => {
                PROCESS_MANAGER.push_live_log(&instance_id, line.clone());
                append_log(&log_path, &line).await;
            }
            Ok(GameFrame::Exit { code }) => {
                exit_code = Some(code);
                break;
            }
            Err(error) => {
                super::android_log(
                    "aml-ipc",
                    &format!("unparsable frame from game: {error}"),
                );
            }
        }
    }

    let code = exit_code.unwrap_or(-1);
    let status = format!("[AML] Process exited (code {code})");
    PROCESS_MANAGER.push_live_log(&instance_id, status.clone());
    append_log(&log_path, &status).await;

    REMOTE_GAMES.write().unwrap().remove(&uuid);
    emit_process(ProcessEvent {
        instance_id,
        uuid,
        event: "finished".into(),
        message: "Exited process".into(),
    });
    let _ = tokio::fs::remove_file(&socket_path).await;
}

fn announce_failed_connect(
    uuid: &str,
    instance_id: &str,
    message: String,
    socket_path: &Path,
) {
    PROCESS_MANAGER.push_live_log(instance_id, format!("[AML] {message}"));
    REMOTE_GAMES.write().unwrap().remove(uuid);
    emit_process(ProcessEvent {
        instance_id: instance_id.to_string(),
        uuid: uuid.to_string(),
        event: "finished".into(),
        message: "Exited process".into(),
    });
    let _ = std::fs::remove_file(socket_path);
}

async fn append_log(path: &Path, line: &str) {
    let mut file = match tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await
    {
        Ok(file) => file,
        Err(_) => return,
    };
    let _ = file.write_all(line.as_bytes()).await;
    let _ = file.write_all(b"\n").await;
}

/// Kill the in-process game via its reported pid (same UID, so `kill` is
/// permitted). Returns false if the session or pid is unknown.
pub(crate) fn kill_remote(instance_id: &str) -> bool {
    let games = REMOTE_GAMES.read().unwrap();
    for game in games.values() {
        if game.instance_id == instance_id {
            if let Some(pid) = *game.pid.read().unwrap() {
                // Safe: kill is a simple bionic/libc syscall wrapper; the pid
                // belongs to our own UID.
                unsafe {
                    kill(pid as i32, SIGKILL);
                }
                return true;
            }
        }
    }
    false
}

/// Metadata for all in-process game sessions.
pub(crate) fn list_remote() -> Vec<ProcessMetadata> {
    REMOTE_GAMES
        .read()
        .unwrap()
        .values()
        .map(|game| ProcessMetadata {
            uuid: game.uuid.clone(),
            instance_id: game.instance_id.clone(),
        })
        .collect()
}
