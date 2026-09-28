//! JRE native-library loader (Phase 3).
//!
//! Loads the JRE's `.so` files from a full, unflattened JRE tree shipped in the
//! app files dir (`<files>/jre`) and resolves `JNI_CreateJavaVM`. Keeping the
//! standard `lib/server/libjvm.so` layout is important: HotSpot computes its
//! boot class path from the *location of libjvm.so* ([os_linux `os::get_boot_path`]
//! walks up to the JRE root and opens `lib/modules`), so java.home must be
//! derivable from where we dlopen the VM from. The dependencies load in strict
//! order:
//!
//!   1. `libjsig.so` — installs HotSpot's signal-chaining first so the VM's own
//!      SIGSEGV-based null checks don't clash with ART's.
//!   2. `libjli.so` — launcher library the JVM bootstrapping relies on.
//!   3. `libjvm.so` — exports `JNI_CreateJavaVM`.
//!
//! All loads use `RTLD_NOW | RTLD_GLOBAL`: the VM's runtime deps must resolve
//! their symbols globally, and the handles are deliberately never released —
//! the JVM is resident for the process lifetime. Alignment is checked at best
//! effort (Android 15+ requires 16 KB pages).

use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::path::Path;

use super::RuntimeError;

const RTLD_NOW: c_int = 2;
const RTLD_GLOBAL: c_int = 0x0100;

// bionic exports dl* from libc; `link dl` keeps the dependency explicit.
#[link(name = "dl")]
unsafe extern "C" {
    fn dlopen(filename: *const c_char, flag: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn dlerror() -> *mut c_char;
}

/// Pointer to the resolved `JNI_CreateJavaVM` invocation entry point.
pub(super) type CreateJavaVmFn = unsafe extern "system" fn(
    pvm: *mut *mut jni::sys::JavaVM,
    penv: *mut *mut c_void,
    args: *mut c_void,
) -> jni::sys::jint;

/// dlopen with `RTLD_NOW | RTLD_GLOBAL`, returning the handle.
unsafe fn load_rtl_global(path: &str) -> Result<*mut c_void, String> {
    let c_path = CString::new(path).map_err(|e| e.to_string())?;
    let handle = dlopen(c_path.as_ptr(), RTLD_NOW | RTLD_GLOBAL);
    if handle.is_null() {
        Err(format!("dlopen({path}): {}", last_error()))
    } else {
        Ok(handle)
    }
}

fn last_error() -> String {
    unsafe {
        let p = dlerror();
        if p.is_null() {
            String::new()
        } else {
            CStr::from_ptr(p).to_string_lossy().into_owned()
        }
    }
}

/// Best-effort check that a given ELF's executable segments satisfy 16 KB page
/// alignment. Logs only — never fails the boot (older targets permit 4 KB).
/// Reads only the ELF headers, not the whole file.
fn check_16k_alignment(path: &Path) {
    use std::fs::File;
    use std::io::Read;
    let Ok(mut f) = File::open(path) else { return };
    // Read enough for the program-header table; most JRE libs < 1 MB.
    const HDR: usize = 4096;
    let mut buf = Vec::with_capacity(HDR);
    let mut h = [0u8; HDR];
    let Ok(read) = f.read(&mut h) else { return };
    buf.extend_from_slice(&h[..read]);
    if buf.len() < 52 {
        return;
    }
    // ELF64 header: e_phoff@0x20(8), e_phentsize@0x36(2), e_phnum@0x38(2).
    let phoff = u64::from_le_bytes(buf[0x20..0x28].try_into().unwrap()) as usize;
    let phentsize = u16::from_le_bytes(buf[0x36..0x38].try_into().unwrap()) as usize;
    let phnum = u16::from_le_bytes(buf[0x38..0x3a].try_into().unwrap()) as usize;
    if phoff == 0 || phentsize == 0 || phnum == 0 {
        return;
    }
    let table_end = phoff + phnum * phentsize;
    if table_end > buf.len() {
        // Header table spills past our snapshot; grow once.
        f.read_to_end(&mut buf).ok();
        if table_end > buf.len() {
            return;
        }
    }
    let mut worst = 0usize;
    for i in 0..phnum {
        let off = phoff + i * phentsize;
        if off + 56 > buf.len() {
            break;
        }
        // p_type@+0, p_align@+32 (PT_LOAD=1, p_flags at +4)
        let p_type = u32::from_le_bytes(buf[off..off + 4].try_into().unwrap());
        if p_type != 1 {
            continue;
        }
        let p_align =
            u64::from_le_bytes(buf[off + 32..off + 40].try_into().unwrap()) as usize;
        worst = worst.max(p_align);
    }
    super::super::android_log(
        "aml-loader",
        &format!(
            "{}: worst p_align=0x{worst:x} ({})",
            path.file_name().unwrap_or_default().to_string_lossy(),
            if worst >= 0x4000 { "16K aligned" } else { "below 16K" },
        ),
    );
}

/// Loaded JRE library handles, kept alive for the process lifetime.
pub(super) struct LoadedJre {
    _handles: Vec<*mut c_void>,
}

// SAFETY: handles are dlopen pointers valid for the process lifetime. We never
// dereference them after load and never release them, so a shared static holding
// them is sound.
unsafe impl Send for LoadedJre {}
unsafe impl Sync for LoadedJre {}

/// Load the JRE libraries and return the `JNI_CreateJavaVM` entry point plus
/// the handles that must stay resident.
///
/// # Safety
///
/// Runs once, before any Java code executes in this process. `jre_root` is the
/// absolute path to the JRE image (`<files>/jre`) laid out as a standard JDK:
/// `lib/server/libjvm.so`, `lib/libjli.so`, `lib/libjsig.so`, `lib/modules`, ...
pub(super) unsafe fn load(jre_root: &str) -> Result<(LoadedJre, CreateJavaVmFn), RuntimeError> {
    // Strict order: jsig (signal chaining) → jli (launcher) → jvm (VM itself).
    // The VM must live at `lib/server/` so HotSpot can derive java.home from it.
    let libs = [
        format!("{jre_root}/lib/libjsig.so"),
        format!("{jre_root}/lib/libjli.so"),
        format!("{jre_root}/lib/server/libjvm.so"),
    ];
    let mut handles = Vec::with_capacity(3);
    for (name, path) in ["libjsig.so".to_string(), "libjli.so".to_string(), "libjvm.so".to_string()]
        .into_iter()
        .zip(libs.iter())
    {
        if !Path::new(path).exists() {
            return Err(RuntimeError::Load(format!("missing {path}")));
        }
        let h = load_rtl_global(path)
            .map_err(|e| RuntimeError::Load(format!("{name}: {e}")))?;
        handles.push(h);
    }
    check_16k_alignment(Path::new(&format!("{jre_root}/lib/server/libjvm.so")));
    let jvm = *handles.last().unwrap();

    // Preload the remaining JDK JNI libraries by ABSOLUTE PATH in dependency
    // order, all RTLD_GLOBAL. The app's linker namespace (clns-*) resolves
    // DT_NEEDED entries by soname through its own search path, which contains
    // the APK nativeLibraryDir (carrying a JDK 17 set of the same SONAMEs) but
    // NOT `<jre>/lib`. Without preloading, e.g. libnio.so's `DT_NEEDED
    // libnet.so` silently binds the JDK 17 APK copy, whose JNI code expects
    // old class layouts (`InetAddress.preferIPv6Address`) and crashes the
    // JDK 21 VM. Preloading the matching copies first makes every by-soname
    // resolution land in the same JRE tree.
    //
    // Only core/runtime libs are listed: AWT/Swing/font libs are intentionally
    // excluded (headless Minecraft does not use them, and libfreetype.so must
    // keep resolving to the GL4ES/LWJGL bridge copy).
    const JDK_PRELOAD: &[&str] = &[
        "libverify",
        "libjava",
        "libzip",
        "libjimage",
        "libnet",
        "libnio",
        "libextnet",
        "libprefs",
        "libsctp",
        "librmi",
        "libmanagement",
        "libmanagement_ext",
        "libmanagement_agent",
        "libinstrument",
        "libdt_socket",
        "libjdwp",
        "libattach",
        "libsyslookup",
    ];
    let mut preloaded = Vec::new();
    for stem in JDK_PRELOAD {
        let path = format!("{jre_root}/lib/{stem}.so");
        if !Path::new(&path).exists() {
            continue;
        }
        match load_rtl_global(&path) {
            Ok(handle) => preloaded.push(handle),
            Err(error) => {
                // Non-fatal: not every JRE ships every entry, and unrelated
                // optional components (e.g. management agents) must not block
                // the VM. Core binding failures surface again at JNI use.
                super::super::android_log(
                    "aml-loader",
                    &format!("optional preload {stem}.so failed: {error}"),
                );
            }
        }
    }
    handles.extend(preloaded);

    let create_fn = dlsym(
        jvm,
        b"JNI_CreateJavaVM\0".as_ptr() as *const c_char,
    );
    if create_fn.is_null() {
        return Err(RuntimeError::Load(format!(
            "JNI_CreateJavaVM not exported by libjvm.so; dlerror={}",
            last_error()
        )));
    }
    let create_fn = std::mem::transmute::<*mut c_void, CreateJavaVmFn>(create_fn);
    Ok((LoadedJre { _handles: handles }, create_fn))
}