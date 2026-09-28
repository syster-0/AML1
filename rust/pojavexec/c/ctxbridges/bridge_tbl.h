// Curated version of the bridge dispatch table.
//
// Provenance: PojavLauncherTeam/PojavLauncher (branch v3_openjdk)
// app_pojavlauncher/src/main/jni/ctxbridges/bridge_tbl.h — GPL-3.0.
// The OSMesa table/includes were removed (AML uses the GL4ES/EGL path only).

#ifndef POJAVLAUNCHER_BRIDGE_TBL_H
#define POJAVLAUNCHER_BRIDGE_TBL_H

#include <ctxbridges/common.h>
#include <ctxbridges/gl_bridge.h>

typedef basic_render_window_t* (*br_init_context_t)(basic_render_window_t* share);
typedef void (*br_make_current_t)(basic_render_window_t* bundle);
typedef basic_render_window_t* (*br_get_current_t)();

bool (*br_init)() = NULL;
br_init_context_t br_init_context = NULL;
br_make_current_t br_make_current = NULL;
br_get_current_t br_get_current = NULL;
void (*br_swap_buffers)() = NULL;
void (*br_setup_window)() = NULL;
void (*br_swap_interval)(int swapInterval) = NULL;

void set_gl_bridge_tbl() {
    br_init = gl_init;
    br_init_context = (br_init_context_t) gl_init_context;
    br_make_current = (br_make_current_t) gl_make_current;
    br_get_current = (br_get_current_t) gl_get_current;
    br_swap_buffers = gl_swap_buffers;
    br_setup_window = gl_setup_window;
    br_swap_interval = gl_swap_interval;
}

#endif //POJAVLAUNCHER_BRIDGE_TBL_H
