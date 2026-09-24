//! ART boundary for the `:game` process (`GameBridge`).
//!
//! These are `extern "system"` entry points whose symbol names match the JNI
//! name-mangling convention for `com.astral.aml.game.GameBridge`, so ART
//! resolves them without an explicit `RegisterNatives`. Their only job is to
//! convert at the boundary and forward into `surface`/runtime state — no game
//! logic, and (critically) the ART JNIEnv never crosses into the game VM.
//!
//! Every entry point is panic-guarded: an unwind across the JNI frame is UB,
//! so a `catch_unwind` confines any Rust panic to log noise instead of tearing
//! down ART.

use std::panic;

use jni::sys::{jclass, jobject, jstring, JNIEnv, JavaVM};

use super::{android_log, JavaVM as AmlVm};

/// Captured ART `JavaVM*`; used later to attach render/runtime threads that
/// call back into Kotlin. Stored once at load time — never per-call.
///
/// Phase 2 only needs the surface window, so this is reserved for Phase 3+.
struct ArtVm(*mut JavaVM);

// OK: `JavaVM*` is an opaque ART handle, valid for the process lifetime. We
// never dereference it; it is only handed back to JNI attach helpers.
unsafe impl Send for ArtVm {}
unsafe impl Sync for ArtVm {}

static ART_VM: std::sync::OnceLock<ArtVm> = std::sync::OnceLock::new();

/// Stash the ART JavaVM for future thread attach. The value is process-lifetime
/// and we never dereference it here, only hand it to JNI attach helpers later.
fn store_vm(vm: *mut JavaVM) {
    let _ = ART_VM.set(ArtVm(vm));
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_astral_aml_game_GameBridge_nativeSurfaceCreated(
    env: JNIEnv,
    _class: jclass,
    surface: jobject,
) {
    // ART's first arg for a JNI method is `JNIEnv*`, whose value is the
    // address of the function-table pointer. jni_sys models it as a single
    // `*const JNINativeInterface_`; ANativeWindow wants that same address
    // typed as `JNIEnv**`. Recasting `env` (not `&env`, which adds a level)
    // preserves the bits `get_raw()` would produce.
    let _ = panic::catch_unwind(|| {
        super::surface::attach(env as *mut jni::sys::JNIEnv, surface)
    });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_astral_aml_game_GameBridge_nativeSurfaceDestroyed(
    _env: JNIEnv,
    _class: jclass,
) {
    let _ = panic::catch_unwind(super::surface::detach);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_astral_aml_game_GameBridge_nativeSendInput(
    _env: JNIEnv,
    _class: jclass,
    _event_json: jstring,
) {
    // Input routing lands in Phase 6/10; acknowledge as no-op for now.
    android_log("aml-bridge", "nativeSendInput (no-op in Phase 2)");
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_astral_aml_game_GameBridge_nativeStart(
    _env: JNIEnv,
    _class: jclass,
    manifest: jstring,
    files_dir: jstring,
) {
    let _ = panic::catch_unwind(|| {
        // Extract the borrowed strings; if either fails to parse we bail
        // before spawning anything.
        let read = |s: jstring| -> Option<String> {
            unsafe {
                // Recast JNIEnv* → `*mut sys::JNIEnv` (bit-preserving, same as the
                // other native entry points in this module).
                let mut env =
                    jni::JNIEnv::from_raw(_env as *mut jni::sys::JNIEnv).ok()?;
                let js = jni::objects::JString::from_raw(s);
                let s = env.get_string(&js).ok()?;
                Some(s.into())
            }
        };
        let (manifest, files) = match (read(manifest), read(files_dir)) {
            (Some(m), Some(f)) => (m, f),
            _ => {
                android_log("aml-bridge", "nativeStart: failed to read string args");
                return;
            }
        };
        // The manifest MUST stay inside the app-private files area: a path
        // outside it would let a crafted intent steer the game classpath.
        let root = files.trim_end_matches('/');
        if !manifest.starts_with(&format!("{root}/")) {
            android_log(
                "aml-bridge",
                &format!("rejecting manifest outside private dir: {manifest}"),
            );
            return;
        }
        super::runtime::start(&manifest, &files);
    });
}

/// Polled by `GameActivity`; true once the game main call has returned.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_astral_aml_game_GameBridge_nativeIsFinished(
    _env: JNIEnv,
    _class: jclass,
) -> bool {
    super::runtime::is_finished()
}

// ---------------------------------------------------------------------------
// Name-based registration support
// ---------------------------------------------------------------------------

/// Register name-mangled natives against the ART JVM. With name-based JNI this
/// is mostly a formality (ART resolves symbols lazily), but we still record the
/// `JavaVM*` here for later thread attach, and log that the branch finalized.
pub(crate) fn register_natives(vm: *mut AmlVm) {
    store_vm(vm as *mut JavaVM);
    android_log("aml-bridge", "GameBridge natives registered (:game branch)");
}