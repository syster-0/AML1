package com.astral.aml

import android.app.Activity
import android.os.Bundle
import android.view.ViewGroup
import android.widget.FrameLayout
import com.astral.aml.game.GameBridge
import com.astral.aml.game.GameSurfaceView
import kotlin.concurrent.thread

/**
 * Game-process activity.
 *
 * Hosts the game surface in the dedicated `:game` process. This is a normal
 * `Activity` (NOT a `FlutterActivity`): the Flutter engine stays out of the
 * game process by design. It is `exported=false` and accepts only a launch
 * manifest path inside the app-private files area, so no other app can steer
 * the game classpath.
 *
 * The native library is loaded eagerly (mirroring `MainActivity`) so
 * `JNI_OnLoad` runs in the `:game` process and installs the `GameBridge`
 * natives. JVM boot happens on a Rust-owned thread; this activity finishes
 * itself once the game main call returns.
 */
class GameActivity : Activity() {
    init {
        System.loadLibrary("rust_lib_aml")
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        val manifest = intent?.getStringExtra(MainActivity.EXTRA_MANIFEST)
        if (manifest == null || !manifest.startsWith("${filesDir.absolutePath}/")) {
            // Entry is intent-only; a malformed launch cannot boot a VM.
            finish()
            return
        }

        val root = FrameLayout(this)
        root.addView(
            GameSurfaceView(this),
            ViewGroup.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                ViewGroup.LayoutParams.MATCH_PARENT,
            ),
        )
        setContentView(root)

        // Boot the HotSpot VM on a Rust-owned thread (never the UI thread).
        GameBridge.nativeStart(manifest, filesDir.absolutePath)

        // Finish once the game main call returns so the :game process does not
        // linger on a dead surface.
        thread(name = "aml-game-finish") {
            while (!GameBridge.nativeIsFinished()) {
                Thread.sleep(200)
            }
            runOnUiThread { finish() }
        }
    }
}
