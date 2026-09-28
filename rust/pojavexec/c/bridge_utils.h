// Shared declarations for the curated utility functions.
//
// Provenance: derived from PojavLauncherTeam/PojavLauncher (branch v3_openjdk)
// app_pojavlauncher/src/main/jni/utils.h — GPL-3.0.

#ifndef AML_POJAVEXEC_BRIDGE_UTILS_H
#define AML_POJAVEXEC_BRIDGE_UTILS_H

#include <jni.h>

// Convert a (possibly NULL) jstring between two VM environments.
jstring convertStringJVM(JNIEnv* srcEnv, JNIEnv* dstEnv, jstring srcStr);

// Attach the current thread to `jvm`, returning a usable env (or NULL).
JNIEnv* get_attached_env(JavaVM* jvm);

JNIEXPORT jstring JNICALL
Java_org_lwjgl_glfw_CallbackBridge_nativeClipboard(JNIEnv* env, jclass clazz,
                                                   jint action, jbyteArray copySrc);

#endif //AML_POJAVEXEC_BRIDGE_UTILS_H
