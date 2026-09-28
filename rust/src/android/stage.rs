//! Staging of the Android bridge native libraries into each version's
//! natives directory.
//!
//! Modern Minecraft/LWJGL resolves JNI libraries through
//! `-Djava.library.path=<natives dir>` (see `android::launch`). The glibc
//! natives shipped inside LWJGL's Maven jars cannot run on Bionic, so the
//! directory must instead carry the app-built Android copies:
//!
//! - GL4ES (`libgl4es_114.so`) — OpenGL→GLES translation;
//! - OpenAL Soft (`libopenal.so`) — audio (OpenSL backend);
//! - LWJGL bindings (`liblwjgl*.so`) and the LWJGL-bundled FreeType build;
//! - `libpojavexec.so` — the EGL/input bridge built by `rust/pojavexec`;
//! - `libjnidispatch.so` — the Bionic-built JNA 7.0.0 dispatch (the jar's
//!   own copy is glibc and fails with `libc.so.6 not found`).
//!
//! All of them ship inside the APK under `jniLibs` (useLegacyPackaging keeps
//! them as real files in `nativeLibraryDir`). They are copied — never symlinked
//! or deleted — because LWJGL/JNA also use the directory as a scratch area.
//!
//! This module runs in the **main** process during launch preparation, before
//! the `:game` process starts HotSpot.

use std::path::{Path, PathBuf};

/// Bridge libraries staged into every per-version natives directory.
///
/// AWT/Swing libs and the JDK's own `libjava`/`libnet`/... set are
/// deliberately absent: those must never be resolved from `java.library.path`
/// (see `runtime::loader` for the matching-JRE preload that prevents the
/// JDK 17 APK copies from shadowing a selected JDK 21 at `DT_NEEDED` time).
pub(super) const BRIDGE_LIBS: &[&str] = &[
    "libgl4es_114.so",
    "libopenal.so",
    "libfreetype.so",
    "liblwjgl.so",
    "liblwjgl_nanovg.so",
    "liblwjgl_opengl.so",
    "liblwjgl_stb.so",
    "liblwjgl_tinyfd.so",
    "libpojavexec.so",
    "libjnidispatch.so",
];

/// Locate the APK `nativeLibraryDir` by inspecting our own mappings. The
/// directory contains the cdylib this code lives in; no JNI round-trip to
/// `ApplicationInfo` is needed.
fn native_library_dir() -> Option<PathBuf> {
    let maps = std::fs::read_to_string("/proc/self/maps").ok()?;
    // Prefer the bridge lib (always a jniLibs entry); fall back to the cdylib.
    for marker in ["libpojavexec.so", "librust_lib_aml.so"] {
        for line in maps.lines() {
            // Mapping rows look like:
            //   7c.. r-xp 00003000 fe:37 139334  /data/app/.../lib/x86_64/libfoo.so
            // Anonymous rows have no path field — skip them.
            let Some(path) = line.split_whitespace().nth(5) else {
                continue;
            };
            if Path::new(path).file_name().is_some_and(|name| name == marker) {
                return Path::new(path).parent().map(Path::to_path_buf);
            }
        }
    }
    None
}

/// Copy missing/outdated bridge libraries from the APK into [natives_root].
///
/// A library is (re)staged when the destination is absent or its size differs
/// from the APK copy. Missing APK entries are skipped without failing the
/// launch: optional/ABI-specific artifacts may legitimately be absent, and a
/// hard failure here would block every launch.
pub(crate) async fn ensure_bridge_natives(natives_root: &Path) {
    let Some(source_dir) = native_library_dir() else {
        super::android_log(
            "aml-stage",
            "could not locate nativeLibraryDir; bridge natives not staged",
        );
        return;
    };

    for name in BRIDGE_LIBS {
        let source = source_dir.join(name);
        let destination = natives_root.join(name);
        let needs_copy = match (tokio::fs::metadata(&source).await, tokio::fs::metadata(&destination).await)
        {
            (Ok(src_meta), Ok(dst_meta)) => src_meta.len() != dst_meta.len(),
            (Ok(_), Err(_)) => true,
            (Err(_), _) => continue,
        };
        if needs_copy {
            match tokio::fs::copy(&source, &destination).await {
                Ok(bytes) => super::android_log(
                    "aml-stage",
                    &format!("staged {name} ({bytes} bytes) from {}", source.display()),
                ),
                Err(error) => super::android_log(
                    "aml-stage",
                    &format!("failed to stage {name}: {error}"),
                ),
            }
        }
    }
}
