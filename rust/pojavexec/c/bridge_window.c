// EGL-facing entry points, curated from the upstream egl_bridge.c.
//
// Provenance: PojavLauncherTeam/PojavLauncher (branch v3_openjdk)
// app_pojavlauncher/src/main/jni/egl_bridge.c — GPL-3.0.
// Everything unrelated to the GL4ES/EGL path (OSMesa, Vulkan/Zink, Turnip,
// JREUtils JNI surface helpers) was removed.

#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include <android/native_window.h>
#include <android/hardware_buffer.h>

#include <environ/environ.h>
#include <ctxbridges/bridge_tbl.h>
#include "bridge_utils.h"

// Defined in input_bridge_v3.c; reports the new monitor size to the GLFW
// stub through the game thread's attached env.
void updateMonitorSize(int width, int height);

// Entry points are resolved by the Java GLFW stub through dlsym; keep them
// in the dynamic symbol table even though nothing links against them at build.
#define EXTERNAL_API __attribute__((used))

/* Inject the ANativeWindow owned by the Rust surface bridge. This replaces
 * PojavLauncher's JREUtils.setupBridgeWindow: AML already holds the window
 * pointer, so there is no JNI Surface conversion. A NULL window marks that
 * the surface was destroyed. */
EXTERNAL_API void pojav_set_bridge_window(struct ANativeWindow *window) {
    pojav_environ->pojavWindow = window;
}

/* Select the GL4ES dispatch table and initialize the EGL display. */
static void pojav_init_opengl(void) {
    // Upstream reads this without a NULL guard (would crash on strcmp(NULL));
    // keep the same behavior but defend against an unset variable.
    const char *forceVsync = getenv("FORCE_VSYNC");
    if (forceVsync != NULL && strcmp(forceVsync, "true") == 0) {
        pojav_environ->force_vsync = true;
    }
    set_gl_bridge_tbl();
    if (br_init()) {
        br_setup_window();
    }
}

EXTERNAL_API int pojavInit(void) {
    pojav_environ->glfwThreadVmEnv = get_attached_env(pojav_environ->runtimeJavaVMPtr);
    if (pojav_environ->glfwThreadVmEnv == NULL) {
        printf("Failed to attach Java-side JNIEnv to GLFW thread\n");
        return 0;
    }
    struct ANativeWindow *window = pojav_environ->pojavWindow;
    if (window == NULL) {
        printf("EGLBridge: no bridge window attached\n");
        return 0;
    }
    ANativeWindow_acquire(window);
    pojav_environ->savedWidth = ANativeWindow_getWidth(window);
    pojav_environ->savedHeight = ANativeWindow_getHeight(window);
    ANativeWindow_setBuffersGeometry(window,
                                     pojav_environ->savedWidth,
                                     pojav_environ->savedHeight,
                                     AHARDWAREBUFFER_FORMAT_R8G8B8X8_UNORM);
    // We are inside the stub's glfwInit() call on the game thread: resolve the
    // GLFW stub class in the calling frame's loader (system for vanilla, Knot
    // for Fabric) before any static method touches it.
    pojav_ensure_runtime_classes(pojav_environ->glfwThreadVmEnv);
    updateMonitorSize(pojav_environ->savedWidth, pojav_environ->savedHeight);
    pojav_init_opengl();
    return 1;
}

EXTERNAL_API void pojavTerminate(void) {
    // gl_bridge owns the EGL display; tear it down there.
    gl_terminate();
}

EXTERNAL_API void* pojavGetCurrentContext(void) {
    return br_get_current();
}

EXTERNAL_API void* pojavCreateContext(void* contextSrc) {
    return br_init_context((basic_render_window_t*)contextSrc);
}

EXTERNAL_API void pojavMakeCurrent(void* window) {
    br_make_current((basic_render_window_t*)window);
}

EXTERNAL_API void pojavSwapBuffers(void) {
    br_swap_buffers();
}

EXTERNAL_API void pojavSwapInterval(int interval) {
    br_swap_interval(interval);
}

/* The surface bridge window changed (a new ANativeWindow, or NULL when the
 * surface was destroyed). Wake the GL bridge so the next swap rebinds to the
 * new window surface, or falls back to a 1x1 pbuffer. */
EXTERNAL_API void pojavNotifyWindow(void) {
    // The dispatch table is only populated by pojavInit on the game VM; the
    // ART side may notify about a window before that (or after terminate).
    if (br_setup_window != NULL) br_setup_window();
}

EXTERNAL_API void pojavSetWindowHint(int hint, int value) {
    // The backend is fixed to GL4ES, so accept every hint; the upstream
    // abort() on unknown APIs must never take the game down.
    (void)hint;
    (void)value;
}
