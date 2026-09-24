//! Game-process native surface handling (Phase 2).
//!
//! Bridges the ART `Surface` handed in by `GameSurfaceView` into a Rust-held
//! `ANativeWindow`, then attaches it on a dedicated render thread. The thread
//! software-fills each frame via `NativeWindow::lock` as a standing proof that
//! the window is really acquired and composable — before any GL backend exists
//! (Phase 5). Surface destroy/recreate must round-trip without crashing: the
//! `Detach` event only releases the window, never tears down shared state, so a
//! recreated surface can attach again.

use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::OnceLock;
use std::thread::{self, JoinHandle};

use jni::sys::jobject;
use ndk::native_window::{NativeWindow, NativeWindowBufferLockGuard};

/// Whether the emulator/system actually attached our payload. Used only to log
/// the observed frame count; not part of game logic.
const TARGET_COLOR: u8 = 0x4c; // keep fill obvious against the dark theme

enum Event {
    /// A new `ANativeWindow` acquired from ART's `Surface`.
    Attach(NativeWindow),
    /// `surfaceDestroyed`: release the current window, keep the thread alive.
    Detach,
}

/// Lazily spawned render thread. Owns the current window across attaches.
static RENDER: OnceLock<Sender<Event>> = OnceLock::new();
static THREAD: OnceLock<JoinHandle<()>> = OnceLock::new();

fn android_log(tag: &str, msg: &str) {
    // Reuse the raw log FFI declared in the parent `android` module.
    super::android_log(tag, msg);
}

fn render_loop(rx: Receiver<Event>) {
    android_log("aml-gfx", "render thread started");
    let mut window: Option<NativeWindow> = None;
    while let Ok(event) = rx.recv() {
        match event {
            Event::Attach(win) => {
                let (w, h) = (win.width(), win.height());
                android_log(
                    "aml-gfx",
                    &format!("attach window {}x{} (@{:p})", w, h, win.ptr().as_ptr()),
                );
                // Draw once per attach: proves ownership + composability without
                // a frame loop, which keeps the thread trivial until Phase 5.
                draw(&win);
                window = Some(win);
            }
            Event::Detach => {
                // Dropping the `NativeWindow` releases the underlying
                // `ANativeWindow`; the thread survives for the next attach.
                android_log("aml-gfx", "detach window");
                window = None;
            }
        }
    }
    android_log("aml-gfx", "render thread stopped");
}

/// Software-fill the window buffer with a solid color so the attach is
/// externally visible. Requires the surface holder to have a known format.
fn draw(win: &NativeWindow) {
    match win.lock(None) {
        Ok(mut guard) => fill(&mut guard),
        Err(e) => android_log("aml-gfx", &format!("lock failed: {e}")),
    }
}

fn fill(guard: &mut NativeWindowBufferLockGuard<'_>) {
    let (w, h) = (guard.width(), guard.height());
    let Some(bytes) = guard.bytes() else {
        android_log("aml-gfx", "no contiguous byte view (unsupported format)");
        return;
    };
    let n = bytes.len();
    if n == 0 {
        return;
    }
    // RGBA8888 layout: write (r, g, b, a). A flat stride over the whole buffer is
    // enough for a solid fill; per-line padding is irrelevant for a uniform color.
    let mut i = 0;
    while i + 3 < n {
        bytes[i] = std::mem::MaybeUninit::new(TARGET_COLOR);
        bytes[i + 1] = std::mem::MaybeUninit::new(TARGET_COLOR);
        bytes[i + 2] = std::mem::MaybeUninit::new(TARGET_COLOR);
        bytes[i + 3] = std::mem::MaybeUninit::new(0xff);
        i += 4;
    }
    android_log("aml-gfx", &format!("filled {}x{} ({n} bytes)", w, h,));
}

/// Ensures the render thread is running, returning the global event sender.
fn sender() -> &'static Sender<Event> {
    RENDER.get_or_init(|| {
        let (tx, rx) = channel();
        let handle = thread::Builder::new()
            .name("aml-gfx".to_string())
            .spawn(move || render_loop(rx))
            .expect("spawn render thread");
        THREAD.set(handle).ok();
        tx
    })
}

/// Called from `Java_..._GameBridge_nativeSurfaceCreated`.
pub fn attach(env: *mut jni::sys::JNIEnv, surface: jobject) {
    sender();
    let win = unsafe { NativeWindow::from_surface(env, surface) };
    match win {
        Some(win) => {
            if let Err(e) = sender().send(Event::Attach(win)) {
                android_log("aml-gfx", &format!("attach send failed: {e}"));
            }
        }
        None => android_log("aml-gfx", "ANativeWindow_fromSurface returned null"),
    }
}

/// Called from `Java_..._GameBridge_nativeSurfaceDestroyed`.
pub fn detach() {
    if let Some(tx) = RENDER.get() {
        if tx.send(Event::Detach).is_err() {
            android_log("aml-gfx", "detach send failed (thread gone?)");
        }
    }
}