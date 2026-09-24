//! HotSpot JVM creation and Minecraft bootstrap (Phase 4).
//!
//! Runs on the dedicated `aml-jvm-main` thread spawned by `runtime::start`.
//! Here we:
//!
//! 1. dlopen the JRE `.so` from the staged JRE tree (`loader::load`).
//! 2. Connect the IPC channel back to the launcher (`log`).
//! 3. Build JNI init args: VM housekeeping properties plus the exact JVM
//!    arguments assembled by `args.rs` (classpath, `-Xmx`, log4j, module
//!    `--add-opens`, ...).
//! 4. Call `JNI_CreateJavaVM`. The returned JNIEnv belongs to HotSpot only
//!    and is never handed to ART (and vice-versa).
//! 5. Resolve the Minecraft main class (`net.minecraft.client.main.Main`),
//!    build its `String[]` arguments and invoke `main`. Failure at window /
//!    GL initialization is expected in Phase 4 and reported like any other
//!    JVM exception — reaching the main class is the acceptance point.

use std::ffi::{c_void, CString};

use jni::objects::JObject;
use jni::sys::{JavaVMInitArgs, JavaVMOption, JNIEnv};

use super::loader::{self, LoadedJre};
use super::log;
use super::super::launch::LaunchManifest;
use super::RuntimeError;

/// Loaded JRE handles kept alive for the process.
static JRE: std::sync::OnceLock<LoadedJre> = std::sync::OnceLock::new();

/// Boot the VM on the calling (game) thread, then invoke the Minecraft main
/// class declared in [manifest].
pub(super) fn boot(manifest: &LaunchManifest) -> Result<(), RuntimeError> {
    // 1) dlopen libjsig → libjli → libjvm from the JRE tree (RTLD_GLOBAL).
    let (loaded, create_jvm) = unsafe { loader::load(&manifest.jre_home) }?;
    let _ = JRE.set(loaded);

    // 2) IPC channel toward the launcher.
    log::connect(&manifest.ipc_socket)?;
    log::write_line(&format!("aml-jvm: connected to {}", manifest.ipc_socket));

    // 3) JNI init args. The first entries locate the JRE image; everything
    //    after comes from the shared launch-argument builder. Own the strings
    //    alongside the JavaVMOption array so every pointer stays valid for
    //    JNI_CreateJavaVM.
    let mut raw_options: Vec<String> = vec![
        format!("-Djava.home={}", manifest.jre_home),
        format!("-Dsun.boot.library.path={}/lib", manifest.jre_home),
    ];
    raw_options.extend(manifest.jvm_args.iter().cloned());

    let owned: Vec<CString> = raw_options
        .into_iter()
        .map(|s| CString::new(s).expect("no NUL in JVM option"))
        .collect();
    let mut options: Vec<JavaVMOption> = owned
        .iter()
        .map(|c| JavaVMOption {
            optionString: c.as_ptr().cast_mut(),
            extraInfo: std::ptr::null_mut(),
        })
        .collect();

    let init_args = JavaVMInitArgs {
        version: 0x0001_0008, // JNI_VERSION_1_8
        nOptions: options.len() as i32,
        options: options.as_mut_ptr(),
        ignoreUnrecognized: 0,
    };

    let mut pvm: *mut jni::sys::JavaVM = std::ptr::null_mut();
    let mut penv: *mut JNIEnv = std::ptr::null_mut();

    // SAFETY: init_args and its option strings are alive for the call; the VM
    // is uninitialized, so CreateJavaVM owns initialization.
    let rc = unsafe {
        create_jvm(
            &mut pvm as *mut _,
            &mut penv as *mut _ as *mut *mut c_void,
            (&init_args as *const JavaVMInitArgs).cast_mut() as *mut c_void,
        )
    };

    if rc != 0 || pvm.is_null() || penv.is_null() {
        return Err(RuntimeError::Jvm(format!(
            "JNI_CreateJavaVM failed rc={rc}"
        )));
    }
    log::write_line("aml-jvm: JNI_CreateJavaVM OK (OpenJDK HotSpot, not ART)");

    // 4) Run the Minecraft main class.
    let exit_code = match run_minecraft(penv, manifest) {
        Ok(()) => 0,
        Err(error) => {
            log::write_line(&format!("{error}"));
            1
        }
    };
    log::send_exit(exit_code);
    Ok(())
}

/// Find the Minecraft main class and call `main(String[])` with the game
/// arguments. A pending Java exception (e.g. failure inside LWJGL/GL setup)
/// is described through the log channel and reported as an error.
fn run_minecraft(
    penv: *mut JNIEnv,
    manifest: &LaunchManifest,
) -> Result<(), RuntimeError> {
    // SAFETY: penv is a live HotSpot JNIEnv handed back by CreateJavaVM.
    let mut env = unsafe { jni::JNIEnv::from_raw(penv) }
        .map_err(|e| RuntimeError::Jvm(format!("JNIEnv::from_raw: {e}")))?;

    let class = env.find_class(&manifest.main_class).map_err(|_| {
        let detail = describe_pending(&mut env);
        RuntimeError::Jvm(format!("find_class {}: {detail}", manifest.main_class))
    })?;
    log::write_line(&format!("aml-jvm: loaded main class {}", manifest.main_class));

    let args = build_args_array(&mut env, &manifest.game_args)?;
    let call_args = [jni::objects::JValue::Object(&args)];
    env.call_static_method(
        class,
        "main",
        "([Ljava/lang/String;)V",
        &call_args,
    )
    .map_err(|_| {
        let detail = describe_pending(&mut env);
        RuntimeError::Jvm(format!("call Minecraft main: {detail}"))
    })?;

    if env
        .exception_check()
        .map_err(|e| RuntimeError::Jvm(format!("exception_check: {e}")))?
    {
        return Err(RuntimeError::Jvm(format!(
            "Minecraft main threw: {detail}",
            detail = describe_pending(&mut env)
        )));
    }
    Ok(())
}

/// Clear and summarize the pending JVM exception via `Throwable.toString`.
fn describe_pending(env: &mut jni::JNIEnv) -> String {
    let throwable = match env.exception_occurred() {
        Ok(throwable) if !throwable.is_null() => throwable,
        _ => return "Java exception without detail".into(),
    };
    if env.exception_clear().is_err() {
        return "Java exception (clear failed)".into();
    }
    env.call_method(&throwable, "toString", "()Ljava/lang/String;", &[])
        .ok()
        .and_then(|value| value.l().ok())
        .filter(|object| !object.is_null())
        .and_then(|object| {
            let message: jni::objects::JString = object.into();
            // Convert inside this closure: the returned `JavaStr` borrows `env`
            // and cannot outlive the closure body.
            env.get_string(&message)
                .map(|text| text.to_string_lossy().into_owned())
                .ok()
        })
        .unwrap_or_else(|| "Java exception (no message)".into())
}

/// Construct `String[]` from the game argument tokens.
fn build_args_array<'local>(
    env: &mut jni::JNIEnv<'local>,
    tokens: &[String],
) -> Result<JObject<'local>, RuntimeError> {
    let array = env
        .new_object_array(
            tokens.len() as i32,
            "java/lang/String",
            &JObject::null(),
        )
        .map_err(|e| RuntimeError::Jvm(format!("new String[]: {e}")))?;
    for (index, token) in tokens.iter().enumerate() {
        let value = env
            .new_string(token)
            .map_err(|e| RuntimeError::Jvm(format!("new_string: {e}")))?;
        env.set_object_array_element(&array, index as i32, value)
            .map_err(|e| RuntimeError::Jvm(format!("set_object_array_element: {e}")))?;
    }
    Ok(array.into())
}
