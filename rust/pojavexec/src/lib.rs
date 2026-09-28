//! `libpojavexec.so` — the Android GLFW/EGL bridge.
//!
//! This crate carries no Rust code. Its build script compiles a curated
//! subset of the PojavLauncher native bridge (GL4ES/EGL context bridge +
//! input bridge) directly into `libpojavexec.so`; the Java-side GLFW stub
//! resolves the bridge entry points from it.
