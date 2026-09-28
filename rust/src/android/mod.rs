//! Android integration (Phase 0–2).
//!
//! Only compiled for `target_os = "android"`. Exposes:
//! - `JNI_OnLoad`: splits on `/proc/self/cmdline`; captures the ART `JavaVM*`
//!   for later thread attach (Phase 2+ uses it via `bridge.rs`).
//! - `bridge.rs`: ART boundary (`GameBridge` native methods) that only converts
//!   types and forwards — the compiler enforces no ART JNIEnv leaks into game
//!   logic.
//! - `surface.rs`: `ANativeWindow` attach/detach on a dedicated render thread.
//!
//! The same cdylib is loaded into both the main and the `:game` process.
//! The main process initializes FRB; the `:game` process instead installs the
//! `GameBridge` natives.

pub(crate) mod bridge;
// On hosts these Android-only modules compile but are never called.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(crate) mod launch;
pub(crate) mod lwjgl_repack;
pub(crate) mod pojav_bridge;
#[cfg(target_os = "android")]
pub(crate) mod processor;
pub(crate) mod runtime;
pub(crate) mod stage;
pub(crate) mod surface;

use std::ffi::{c_int, c_void, CString};

/// Opaque handle for the JNI `JavaVM*`; captured in `JNI_OnLoad` for later
/// thread attach. Detached from any particular JNIEnv by design.
#[repr(C)]
pub(crate) struct JavaVM {
    // Reserved; the pointer value is handed back to `AttachCurrentThreadAsDaemon`.
    _private: [u8; 0],
}

const JNI_VERSION_1_6: c_int = 0x0001_0006;
const ANDROID_LOG_INFO: c_int = 4;

/// ART `JavaVM*` captured in the MAIN process. `:game` keeps its own handle in
/// `bridge.rs`; the main process needs one to start `ProcessorService` from
/// pure Rust during Forge/NeoForge installs. Stored as `usize` for Send/Sync —
/// the pointer is process-lifetime and only ever handed back to JNI.
#[cfg(target_os = "android")]
static MAIN_ART_VM: std::sync::OnceLock<usize> = std::sync::OnceLock::new();

/// The ART `JavaVM*` captured in the main process, if `JNI_OnLoad` ran there.
#[cfg(target_os = "android")]
pub(crate) fn main_art_vm() -> Option<*mut JavaVM> {
    MAIN_ART_VM.get().map(|vm| *vm as *mut JavaVM)
}

#[link(name = "log")]
unsafe extern "C" {
    fn __android_log_write(prio: c_int, tag: *const std::ffi::c_char, msg: *const std::ffi::c_char)
        -> c_int;
}

/// Log to logcat. `tag`/`msg` are short-lived; borrow ends before the FFI call.
pub(crate) fn android_log(tag: &str, msg: &str) {
    if let (Ok(tag), Ok(msg)) = (CString::new(tag), CString::new(msg)) {
        unsafe {
            __android_log_write(ANDROID_LOG_INFO, tag.as_ptr(), msg.as_ptr());
        }
    }
}

fn probe_branch() -> &'static str {
    let cmdline = std::fs::read("/proc/self/cmdline")
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .unwrap_or_default();
    // Both branches land on the same cdylib; the suffix after the last NUL
    // terminator tells us which process we are.
    if cmdline.split('\0').any(|part| part.ends_with(":game")) {
        ":game"
    } else if cmdline.split('\0').any(|part| part.ends_with(":proc")) {
        ":proc"
    } else {
        "main"
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn JNI_OnLoad(vm: *mut JavaVM, _reserved: *mut c_void) -> c_int {
    let branch = probe_branch();
    android_log(
        "aml-jni",
        &format!(
            "JNI_OnLoad pid={} branch={}",
            std::process::id(),
            branch,
        ),
    );
    match branch {
        // `:game` process: install the GameBridge natives on the ART JVM.
        ":game" => bridge::register_natives(vm),
        // Main process: FRB initializes via Dart; capture the ART VM so the
        // processor runner can start the `:proc` service without a Dart
        // round-trip.
        "main" => {
            #[cfg(target_os = "android")]
            {
                let _ = MAIN_ART_VM.set(vm as usize);
            }
        }
        // `:proc` hosts headless install processors; name-mangled JNI needs
        // no registration and nothing here touches the ART VM afterwards.
        ":proc" => {}
        _ => unreachable!("probe_branch returned a known constant"),
    }
    JNI_VERSION_1_6
}