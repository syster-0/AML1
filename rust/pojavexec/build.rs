// Build the curated C bridge directly into libpojavexec.so.
//
// The bridge is pure C: its entry points are reached through JNI and dlsym,
// not through Rust. Routing it through a rustc cdylib would leave the version
// script without global symbols, hiding (and then GC-collecting) every C
// symbol. Instead, on Android targets this script invokes the NDK compiler
// (CC from the environment, with CFLAGS such as the --target=<triple><api>
// flag) and links the shared object into OUT_DIR directly. It is then
// deployed manually, following the same pattern as the JRE libraries.
//
// Non-Android targets build nothing: the bridge is Android-only.
fn main() {
    let target = std::env::var("TARGET").unwrap_or_default();
    if !target.contains("android") {
        return;
    }

    let root = std::path::PathBuf::from("c");
    let sources = [
        "environ/environ.c",
        "ctxbridges/loader_dlopen.c",
        "ctxbridges/gl_bridge.c",
        "ctxbridges/egl_loader.c",
        "input_bridge_v3.c",
        "bridge_window.c",
        "bridge_utils.c",
    ];

    for src in sources {
        println!("cargo:rerun-if-changed=c/{src}");
    }

    let out_dir = std::env::var("OUT_DIR").unwrap();
    let output = std::path::Path::new(&out_dir).join("libpojavexec.so");

    // CC is the NDK clang; CFLAGS carries the Android --target flag (and any
    // extra flags the builder chooses to pass).
    let cc = std::env::var("CC").unwrap_or_else(|_| "clang".to_string());
    let cflags = std::env::var("CFLAGS").unwrap_or_default();

    let mut cmd = std::process::Command::new(cc);
    cmd.arg("-shared")
        .arg("-fPIC")
        .arg("-O2")
        .arg("-Wall")
        .arg("-Wl,--gc-sections")
        .arg("-Wl,-z,relro,-z,now")
        .arg("-Wl,-z,noexecstack")
        .arg("-o")
        .arg(&output);
    for flag in cflags.split_whitespace() {
        cmd.arg(flag);
    }
    for src in sources {
        cmd.arg(root.join(src));
    }
    cmd.arg("-I").arg(&root)
        .arg("-llog")
        .arg("-landroid")
        .arg("-ldl");

    let status = cmd.status().expect("failed to invoke C compiler");
    assert!(status.success(), "linking libpojavexec.so failed");

    println!("pojavexec artifact: {}", output.display());
}
