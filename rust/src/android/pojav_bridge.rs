//! Dynamic bindings to `libpojavexec.so` (the LWJGL/EGL bridge).
//!
//! pojavexec is deployed alongside the JNI libraries, but it is built
//! independently and the Rust crate does not link against it; instead this
//! module loads it once with `dlopen` and exposes the handful of entry points
//! the launcher needs. Loading happens on first use — by then the library is
//! guaranteed to be present in the app's native directory.

use std::ffi::{c_void, CString};
use std::sync::OnceLock;

const RTLD_NOW: i32 = 2;
const RTLD_LOCAL: i32 = 0;

unsafe extern "C" {
    fn dlopen(filename: *const std::ffi::c_char, flag: i32) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const std::ffi::c_char) -> *mut c_void;
}

type VmInitFn = extern "C" fn(*mut c_void, *mut c_void);
type SetWindowFn = extern "C" fn(*mut c_void);
type NotifyWindowFn = extern "C" fn();

struct PojavBridge {
    on_art: VmInitFn,
    on_runtime: VmInitFn,
    set_window: SetWindowFn,
    notify_window: NotifyWindowFn,
}

fn load() -> Option<&'static PojavBridge> {
    static BRIDGE: OnceLock<Option<PojavBridge>> = OnceLock::new();
    BRIDGE
        .get_or_init(|| unsafe {
            let name = CString::new("libpojavexec.so").unwrap();
            let handle = dlopen(name.as_ptr(), RTLD_NOW | RTLD_LOCAL);
            if handle.is_null() {
                return None;
            }
            let sym = |symbol: &str| -> *mut c_void {
                let c = CString::new(symbol).unwrap();
                dlsym(handle, c.as_ptr())
            };
            let on_art = sym("pojav_bridge_on_art");
            let on_runtime = sym("pojav_bridge_on_runtime");
            let set_window = sym("pojav_set_bridge_window");
            let notify_window = sym("pojavNotifyWindow");
            if [on_art, on_runtime, set_window, notify_window]
                .iter()
                .any(|p| p.is_null())
            {
                return None;
            }
            Some(PojavBridge {
                on_art: std::mem::transmute(on_art),
                on_runtime: std::mem::transmute(on_runtime),
                set_window: std::mem::transmute(set_window),
                notify_window: std::mem::transmute(notify_window),
            })
        })
        .as_ref()
}

/// Register the ART VM and the app-side `CallbackBridge` with the bridge.
/// `env` must originate from a Java frame so class lookup finds app classes.
pub(crate) fn init_art(vm: *mut c_void, env: *mut c_void) -> bool {
    match load() {
        Some(bridge) => {
            (bridge.on_art)(vm, env);
            true
        }
        None => false,
    }
}

/// Register the HotSpot VM and the GLFW stub class with the bridge.
pub(crate) fn init_runtime(vm: *mut c_void, env: *mut c_void) -> bool {
    match load() {
        Some(bridge) => {
            (bridge.on_runtime)(vm, env);
            true
        }
        None => false,
    }
}

/// Set the current bridge ANativeWindow (NULL marks surface destruction).
pub(crate) fn set_window(window: *mut c_void) -> bool {
    match load() {
        Some(bridge) => {
            (bridge.set_window)(window);
            true
        }
        None => false,
    }
}

/// Ask the GL bridge to rebind to the (newly set) bridge window next swap.
pub(crate) fn notify_window() -> bool {
    match load() {
        Some(bridge) => {
            (bridge.notify_window)();
            true
        }
        None => false,
    }
}
