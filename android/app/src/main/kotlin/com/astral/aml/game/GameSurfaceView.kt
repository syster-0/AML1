package com.astral.aml.game

import android.content.Context
import android.view.Surface
import android.view.SurfaceHolder
import android.view.SurfaceView

/**
 * Game-process surface host.
 *
 * Owns a [SurfaceView] and mirrors its surface lifecycle into the native game
 * side. `surfaceDestroyed` is not called destructively: the Rust render thread
 * keeps running and merely detaches, so a recreated surface (rotation, lock
 * screen, Activity recreate) can re-attach without a full teardown.
 *
 * Phase 2 touches only the surface plumbing; touch input is forwarded verbatim
 * later (Phase 6/10) — `onTouchEvent` is intentionally kept as-is for now.
 */
class GameSurfaceView(context: Context) : SurfaceView(context) {
    init {
        // The holder must be set before the Surface can be drawn to.
        holder.addCallback(object : SurfaceHolder.Callback {
            override fun surfaceCreated(holder: SurfaceHolder) {
                GameBridge.nativeSurfaceCreated(holder.surface)
            }

            override fun surfaceChanged(
                holder: SurfaceHolder,
                format: Int,
                width: Int,
                height: Int,
            ) {
                // Geometry changes come through in a separate callback; the
                // native attach already happened, nothing to forward here.
            }

            override fun surfaceDestroyed(holder: SurfaceHolder) {
                GameBridge.nativeSurfaceDestroyed()
            }
        })
    }
}