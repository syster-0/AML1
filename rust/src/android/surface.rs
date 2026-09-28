//! Game-process native surface handling (Phase 5).
//!
//! Bridges the ART `Surface` handed in by `GameSurfaceView` into an
//! `ANativeWindow`, then injects that window into `libpojavexec.so`, which
//! owns the EGL display and draws into it through GL4ES. The Rust side holds
//! one reference to the window for the surface lifetime; pojavexec acquires
//! its own references and releases them as surfaces are swapped.
//!
//! Surface destroy/recreate round-trips without crashing: detach hands NULL to
//! pojavexec (it falls back to a 1x1 pbuffer), keeps no global state teardown,
//! and a recreated surface can attach again.

use std::ffi::c_void;
use std::sync::Mutex;

use jni::sys::jobject;
use ndk::native_window::NativeWindow;

/// The window owned by the current ART surface. `None` between destroy and
/// recreate. Lock scope is deliberately tiny everywhere in this module.
static WINDOW: Mutex<Option<NativeWindow>> = Mutex::new(None);

fn android_log(tag: &str, msg: &str) {
    super::android_log(tag, msg);
}

/// Called from `Java_..._GameBridge_nativeSurfaceCreated`.
pub fn attach(env: *mut jni::sys::JNIEnv, surface: jobject) {
    let Some(window) = (unsafe { NativeWindow::from_surface(env, surface) }) else {
        android_log("aml-gfx", "ANativeWindow_fromSurface returned null");
        return;
    };
    let ptr = window.ptr().as_ptr();
    let (width, height) = (window.width(), window.height());

    {
        let mut guard = WINDOW
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = Some(window);
    }

    // First attach also registers the ART VM + app CallbackBridge. `env` here
    // is from a Java frame, so the bridge's FindClass resolves app classes.
    if let Some(vm) = super::bridge::art_vm() {
        if !super::pojav_bridge::init_art(vm as *mut c_void, env as *mut c_void) {
            android_log("aml-gfx", "failed to register ART VM with pojavexec");
        }
    } else {
        android_log("aml-gfx", "attach before ART VM registration");
    }

    if !super::pojav_bridge::set_window(ptr as *mut c_void) {
        android_log("aml-gfx", "failed to set bridge window");
    }
    if !super::pojav_bridge::notify_window() {
        android_log("aml-gfx", "failed to notify bridge window");
    }
    android_log(
        "aml-gfx",
        &format!("window injected {width}x{height} (@{ptr:p})"),
    );
}

/// Called from `Java_..._GameBridge_nativeSurfaceDestroyed`.
pub fn detach() {
    {
        let mut guard = WINDOW
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // Dropping the NativeWindow releases our ANativeWindow reference.
        *guard = None;
    }
    // pojavexec switches its context to a pbuffer at the next swap and
    // releases any reference it held to the destroyed window.
    if !super::pojav_bridge::set_window(std::ptr::null_mut()) {
        android_log("aml-gfx", "failed to clear bridge window");
    }
    if !super::pojav_bridge::notify_window() {
        android_log("aml-gfx", "failed to notify bridge window");
    }
    android_log("aml-gfx", "window detached");
}
