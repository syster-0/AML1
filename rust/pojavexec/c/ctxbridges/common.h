//
// Created by maks on 18.10.2023.
//
// Provenance: PojavLauncherTeam/PojavLauncher (branch v3_openjdk)
// app_pojavlauncher/src/main/jni/ctxbridges/common.h — GPL-3.0.

#ifndef POJAVLAUNCHER_COMMON_H
#define POJAVLAUNCHER_COMMON_H

#define STATE_RENDERER_ALIVE 0
#define STATE_RENDERER_NEW_WINDOW 1

typedef struct {
    char       state;
    struct ANativeWindow *nativeSurface;
    struct ANativeWindow *newNativeSurface;
} basic_render_window_t;

#endif //POJAVLAUNCHER_COMMON_H
