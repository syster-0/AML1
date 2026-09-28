//! Forge/NeoForge install-processor execution on Android.
//!
//! Desktop AML runs each install-profile processor as an external
//! `java -cp ... MainClass args...` process. Android has no `java` binary and
//! SELinux denies executing from the data partition, so each processor instead
//! runs inside a short-lived in-process HotSpot VM hosted by
//! `ProcessorService` in the dedicated `:proc` process (a separate process
//! from `:game` so installs also work while another instance is running).
//!
//! HotSpot allows only one `JNI_CreateJavaVM` per process, and several
//! processor mains call `System.exit` on completion, so one process runs
//! exactly one processor: the service kills its own process once the run
//! finishes and the next processor gets a fresh `:proc`.

use std::path::Path;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;

use super::launch::{GameFrame, LaunchManifest};
use crate::launcher::process::PROCESS_MANAGER;

/// Time waiting for `:proc` to connect back after the service is started.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
/// Hard cap for a single processor (BINPATCH is the slow one on device CPUs).
const PROCESSOR_TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// Serializes processor runs: one `:proc` process hosts one JVM at a time.
static RUN_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
}
const SIGKILL: i32 = 9;

/// Kill a leftover `:proc` process (if any) and wait until it is gone.
///
/// One process hosts at most one HotSpot VM; starting a service while an old
/// `:proc` is still alive (or dying) would let AMS deliver the intent to that
/// process, where the second `JNI_CreateJavaVM` is doomed. Killing from the
/// launcher — never from the service itself — keeps the ordering exact.
fn reap_proc_process() {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if !name.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let Ok(cmdline) = std::fs::read_to_string(entry.path().join("cmdline")) else {
            continue;
        };
        if cmdline.split('\0').next() == Some("com.astral.aml:proc") {
            if let Ok(pid) = name.parse::<i32>() {
                // Safe: kill is a plain libc wrapper; the pid belongs to our
                // own UID (verified by the cmdline match above).
                unsafe {
                    kill(pid, SIGKILL);
                }
            }
        }
    }
}

/// Wait until no `:proc` process exists anymore (post-SIGKILL teardown).
async fn wait_proc_gone() {
    for _ in 0..100 {
        let alive = std::fs::read_dir("/proc")
            .map(|entries| {
                entries.flatten().any(|entry| {
                    std::fs::read_to_string(entry.path().join("cmdline"))
                        .map(|c| c.split('\0').next() == Some("com.astral.aml:proc"))
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false);
        if !alive {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Run one install-profile processor to completion. Returns the processor
/// exit code; a missing/timeout exit maps to -1.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_processor(
    files_root: &Path,
    instance_id: &str,
    working_dir: &Path,
    jre_home: &str,
    classpath: &str,
    main_class: &str,
    args: &[String],
) -> Result<i32> {
    let _permit = RUN_LOCK.lock().await;
    // A previous run's :proc must be fully gone before AMS gets the next
    // start request, otherwise the intent lands in a process whose JVM slot
    // is already spent.
    reap_proc_process();
    wait_proc_gone().await;

    let runtime_dir = files_root.join("runtime");
    let manifest_dir = runtime_dir.join("manifests");
    let ipc_dir = runtime_dir.join("ipc");
    tokio::fs::create_dir_all(&manifest_dir).await?;
    tokio::fs::create_dir_all(&ipc_dir).await?;

    // A short id keeps the socket path well under UNIX_PATH_MAX (108 bytes).
    let id = uuid::Uuid::new_v4().to_string();
    let id: String = id.chars().take(12).collect();
    let manifest_path = manifest_dir.join(format!("proc-{id}.json"));
    let socket_path = ipc_dir.join(format!("proc-{id}.sock"));
    let _ = tokio::fs::remove_file(&socket_path).await;

    let listener = UnixListener::bind(&socket_path)
        .with_context(|| format!("bind IPC socket {}", socket_path.display()))?;

    let manifest = LaunchManifest {
        schema_version: super::launch::SCHEMA_VERSION,
        jre_home: jre_home.to_string(),
        working_dir: working_dir.to_string_lossy().into_owned(),
        jvm_args: vec![
            "-Xmx512m".to_string(),
            "-Djava.awt.headless=true".to_string(),
            "-Dfile.encoding=UTF-8".to_string(),
            format!("-Djava.class.path={classpath}"),
        ],
        main_class: main_class.replace('.', "/"),
        game_args: args.to_vec(),
        env: vec![],
        ipc_socket: socket_path.to_string_lossy().into_owned(),
        headless: true,
    };
    {
        let json = serde_json::to_vec_pretty(&manifest)?;
        let mut file = tokio::fs::File::create(&manifest_path)
            .await
            .with_context(|| format!("create {}", manifest_path.display()))?;
        file.write_all(&json).await?;
        file.sync_all().await?;
    }

    start_processor_service(&manifest_path).context("start ProcessorService")?;

    let label = format!("processor {main_class}");
    let (stream, _) = tokio::time::timeout(CONNECT_TIMEOUT, listener.accept())
        .await
        .map_err(|_| anyhow!("{label}: :proc service did not connect"))?
        .with_context(|| format!("{label}: IPC accept failed"))?;

    let mut lines = BufReader::new(stream).lines();
    let mut exit_code: Option<i32> = None;
    let mut pid: Option<i32> = None;
    let read_loop = async {
        while let Ok(Some(text)) = lines.next_line().await {
            match serde_json::from_str::<GameFrame>(&text) {
                Ok(GameFrame::Hello { pid: p }) => pid = Some(p as i32),
                Ok(GameFrame::Log { line }) => {
                    PROCESS_MANAGER.push_live_log(instance_id, line);
                }
                Ok(GameFrame::Exit { code }) => {
                    exit_code = Some(code);
                    break;
                }
                Err(_) => {}
            }
        }
    };
    if tokio::time::timeout(PROCESSOR_TIMEOUT, read_loop)
        .await
        .is_err()
    {
        PROCESS_MANAGER.push_live_log(instance_id, format!("[AML] {label}: timed out"));
    }

    // One JVM per process: retire :proc now that its run is over (or stuck),
    // and wait for the teardown so the next start gets a fresh process.
    if let Some(pid) = pid {
        unsafe {
            kill(pid, SIGKILL);
        }
    } else {
        reap_proc_process();
    }
    wait_proc_gone().await;

    let _ = tokio::fs::remove_file(&socket_path).await;
    let _ = tokio::fs::remove_file(&manifest_path).await;

    Ok(exit_code.unwrap_or(-1))
}

/// Start `ProcessorService` in the `:proc` process. There is no Dart round
/// trip: the install runs deep inside Rust, so ART is driven directly through
/// the VM captured at `JNI_OnLoad`, with the Application context obtained via
/// `ActivityThread.currentApplication()`.
fn start_processor_service(manifest_path: &Path) -> Result<()> {
    use jni::objects::JValue;
    use jni::JavaVM;

    let vm_ptr = super::main_art_vm().ok_or_else(|| anyhow!("ART VM not captured"))?;
    // SAFETY: the pointer was captured from a live ART `JNI_OnLoad` and the
    // ART VM outlives the process. `JavaVM` has no Drop that would destroy it.
    let vm = unsafe { JavaVM::from_raw(vm_ptr as *mut jni::sys::JavaVM) }
        .map_err(|e| anyhow!("JavaVM::from_raw: {e}"))?;
    let mut env = vm
        .attach_current_thread()
        .map_err(|e| anyhow!("attach_current_thread: {e}"))?;

    let app = env
        .call_static_method(
            "android/app/ActivityThread",
            "currentApplication",
            "()Landroid/app/Application;",
            &[],
        )
        .and_then(|value| value.l())
        .map_err(|e| anyhow!("ActivityThread.currentApplication: {e}"))?;

    let intent = env
        .new_object("android/content/Intent", "()V", &[])
        .map_err(|e| anyhow!("new Intent: {e}"))?;

    let class_name = env
        .new_string("com.astral.aml.game.ProcessorService")
        .map_err(|e| anyhow!("new_string: {e}"))?;
    env.call_method(
        &intent,
        "setClassName",
        "(Landroid/content/Context;Ljava/lang/String;)Landroid/content/Intent;",
        &[JValue::Object(&app), JValue::Object(&class_name)],
    )
    .map_err(|e| anyhow!("Intent.setClassName: {e}"))?;

    let extra_key = env.new_string("manifest").map_err(|e| anyhow!("new_string: {e}"))?;
    let extra_value = env
        .new_string(manifest_path.to_string_lossy().as_ref())
        .map_err(|e| anyhow!("new_string: {e}"))?;
    env.call_method(
        &intent,
        "putExtra",
        "(Ljava/lang/String;Ljava/lang/String;)Landroid/content/Intent;",
        &[JValue::Object(&extra_key), JValue::Object(&extra_value)],
    )
    .map_err(|e| anyhow!("Intent.putExtra: {e}"))?;

    env.call_method(
        &app,
        "startService",
        "(Landroid/content/Intent;)Landroid/content/ComponentName;",
        &[JValue::Object(&intent)],
    )
    .map_err(|e| anyhow!("Context.startService: {e}"))?;

    super::android_log(
        "aml-proc",
        &format!("started ProcessorService for {}", manifest_path.display()),
    );
    Ok(())
}
