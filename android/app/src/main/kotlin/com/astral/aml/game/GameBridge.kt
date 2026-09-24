package com.astral.aml.game

import android.view.Surface

/**
 * Thin surface for the native side of the `:game` process.
 *
 * The `native*` functions are resolved by ART through JNI name-mangling against
 * Rust symbols `Java_com_astral_aml_game_GameBridge_*` (see rust/src/android/
 * bridge.rs), so this class MUST stay in the `com.astral.aml.game` package: it
 * is part of the JNI symbol-name contract, not just a namespace choice.
 *
 * The class deliberately carries no business logic; it only hands the Android
 * [Surface] and the launch manifest across to Rust. Everything else (JRE, VM
 * boot, rendering, input) lives in the game process owned by Rust.
 *
 * Declared as a singleton `object` (not a `companion object`): a `companion`
 * member's JNI symbol would be mangled with a `$Companion` segment and no longer
 * match the Rust `Java_com_astral_aml_game_GameBridge_*` exports.
 */
object GameBridge {
    // `surfaceCreated`: convert the Surface to an ANativeWindow and attach.
    external fun nativeSurfaceCreated(surface: Surface)
    // `surfaceDestroyed`: detach; the render thread survives for the next
    // surface, so a recreated Surface can attach again.
    external fun nativeSurfaceDestroyed()
    // Reserved for Phase 6/10; declared here to keep the JNI surface stable.
    external fun nativeSendInput(eventJson: String)

    // Boot the HotSpot JVM described by `manifest` (private launch manifest
    // written by the launcher). `filesDir` bounds the private-path validation.
    external fun nativeStart(manifest: String, filesDir: String)

    // True once the game main call has returned; GameActivity polls this.
    external fun nativeIsFinished(): Boolean
}
