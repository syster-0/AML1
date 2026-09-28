//
// Created by maks on 17.09.2022.
//
// Provenance: PojavLauncherTeam/PojavLauncher (branch v3_openjdk)
// app_pojavlauncher/src/main/jni/ctxbridges/gl_bridge.h — GPL-3.0.

#include <EGL/egl.h>
#include <stdbool.h>
#ifndef POJAVLAUNCHER_GL_BRIDGE_H
#define POJAVLAUNCHER_GL_BRIDGE_H

typedef struct {
    char       state;
    struct ANativeWindow *nativeSurface;
    struct ANativeWindow *newNativeSurface;
    EGLConfig  config;
    EGLint     format;
    EGLContext context;
    EGLSurface surface;
} gl_render_window_t;

bool gl_init();
gl_render_window_t* gl_get_current();
gl_render_window_t* gl_init_context(gl_render_window_t* share);
void gl_make_current(gl_render_window_t* bundle);
void gl_swap_buffers();
void gl_setup_window();
void gl_swap_interval(int swapInterval);
// AML addition: tear down the EGL display owned by this bridge.
void gl_terminate();

#endif //POJAVLAUNCHER_GL_BRIDGE_H
