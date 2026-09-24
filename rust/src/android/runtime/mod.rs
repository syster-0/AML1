//! Game-process JVM runtime (Phase 4).
//!
//! Boots a real OpenJDK HotSpot VM inside the `:game` process via the JNI
//! Invocation API, completely separate from the ART VM that hosts the Kotlin
//! shell. The two never share a JNIEnv: ART threads stay on the Kotlin side;
//! the HotSpot VM is created and owned on a dedicated game thread (`jvm.rs`).
//! Library loading lives in `loader.rs`; the launcher IPC channel lives in
//! `log.rs`.
//!
//! The launch is fully described by the private launch manifest the launcher
//! process writes (schema in `android::launch`): JRE home, classpath/JVM args,
//! main class and game arguments. The game thread parses it once and hands it
//! to `jvm::boot`.

pub(crate) mod jre;
pub(crate) mod jvm;
pub(crate) mod loader;
pub(crate) mod log;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

use super::android_log;
use super::launch::LaunchManifest;

/// Absolute app-private files dir; captured once at `nativeStart`.
pub(crate) fn set_files_dir(dir: &str) {
    let _ = FILES_DIR.get_or_init(|| dir.trim_end_matches('/').to_string());
}

pub(crate) fn files_dir() -> &'static str {
    FILES_DIR.get_or_init(|| "/placeholder/unset".to_string())
}

static FILES_DIR: OnceLock<String> = OnceLock::new();

/// Set once the game main call has returned, so `GameActivity` can finish
/// itself from its UI thread.
static FINISHED: AtomicBool = AtomicBool::new(false);

pub(crate) fn is_finished() -> bool {
    FINISHED.load(Ordering::Acquire)
}

/// Error type shared across the runtime's bootstrap steps.
#[derive(Debug)]
pub(crate) enum RuntimeError {
    /// A needed library could not be dlopen'd or a symbol was missing.
    Load(String),
    /// The VM failed to start (bad args, overlapping init, etc.).
    Jvm(String),
    /// IPC failure while wiring the launcher channel.
    Log(String),
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RuntimeError::Load(s) => write!(f, "runtime load error: {s}"),
            RuntimeError::Jvm(s) => write!(f, "jvm error: {s}"),
            RuntimeError::Log(s) => write!(f, "log channel error: {s}"),
        }
    }
}

/// Entry point invoked from the ART `GameBridge.nativeStart` boundary. The
/// manifest file is parsed here, then VM boot runs on a fresh thread so
/// JNI_CreateJavaVM never blocks the ART UI thread.
pub(crate) fn start(manifest_path: &str, files_path: &str) {
    set_files_dir(files_path);
    android_log(
        "aml-runtime",
        &format!("start requested manifest={manifest_path}"),
    );

    let manifest = match std::fs::read_to_string(manifest_path) {
        Ok(contents) => contents,
        Err(error) => {
            android_log(
                "aml-runtime",
                &format!("failed to read manifest: {error}"),
            );
            FINISHED.store(true, Ordering::Release);
            return;
        }
    };
    let manifest: LaunchManifest = match serde_json::from_str(&manifest) {
        Ok(manifest) => manifest,
        Err(error) => {
            android_log(
                "aml-runtime",
                &format!("failed to parse manifest: {error}"),
            );
            FINISHED.store(true, Ordering::Release);
            return;
        }
    };

    std::thread::Builder::new()
        .name("aml-jvm-main".to_string())
        .spawn(move || {
            if let Err(error) = jvm::boot(&manifest) {
                android_log("aml-runtime", &format!("jvm boot failed: {error}"));
            }
            FINISHED.store(true, Ordering::Release);
        })
        .expect("failed to spawn aml-jvm-main thread");
}
