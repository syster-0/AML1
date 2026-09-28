//
// Created by maks on 26.10.2024.
//
// Provenance: PojavLauncherTeam/PojavLauncher (branch v3_openjdk)
// app_pojavlauncher/src/main/jni/ctxbridges/loader_dlopen.h — GPL-3.0.

#ifndef POJAVLAUNCHER_LOADER_DLOPEN_H
#define POJAVLAUNCHER_LOADER_DLOPEN_H

void* loader_dlopen(char* primaryName, char* secondaryName, int flags);

#endif //POJAVLAUNCHER_LOADER_DLOPEN_H
