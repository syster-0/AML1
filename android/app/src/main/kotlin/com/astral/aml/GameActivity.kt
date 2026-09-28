package com.astral.aml

import android.app.Activity
import android.content.Intent
import android.os.Bundle
import android.os.Process
import android.view.KeyEvent
import android.view.ViewGroup
import android.widget.FrameLayout
import com.astral.aml.game.GameBridge
import com.astral.aml.game.GameService
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

        // Expose an app context to the ART-side input/clipboard bridge before
        // the surface (and then the game) starts producing events.
        org.lwjgl.glfw.CallbackBridge.attach(this)

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

        // Foreground service keeps the process at foreground priority when the
        // user switches away, and owns the swipe-away kill semantics.
        startForegroundService(Intent(this, GameService::class.java))

        // Once the game main call returns: retire the task AND the process so
        // a dead game VM never lingers (the recents card must disappear too).
        thread(name = "aml-game-finish") {
            while (!GameBridge.nativeIsFinished()) {
                Thread.sleep(200)
            }
            runOnUiThread {
                stopService(Intent(this, GameService::class.java))
                finishAndRemoveTask()
            }
            // Give AMS a moment to tear down the task and the Exit IPC frame
            // time to reach the launcher, then kill the emptied process.
            Thread.sleep(500)
            Process.killProcess(Process.myPid())
        }
    }

    // The Android back key is the game's ESC: it opens/closes screens and the
    // pause menu (which also ungrabs the cursor). Consuming it here also keeps
    // an accidental back press from killing the game Activity.
    override fun onKeyDown(keyCode: Int, event: KeyEvent?): Boolean {
        if (keyCode == KeyEvent.KEYCODE_BACK && event?.repeatCount == 0) {
            org.lwjgl.glfw.CallbackBridge.nativeSendKey(GLFW_KEY_ESCAPE, 0, GLFW_PRESS, 0)
            return true
        }
        return super.onKeyDown(keyCode, event)
    }

    override fun onDestroy() {
        super.onDestroy()
        // The user swiped the card away or the system tore down the task.
        // Without this the :game process would stay alive as a headless VM
        // that the launcher still shows as running.
        stopService(Intent(this, GameService::class.java))
        Process.killProcess(Process.myPid())
    }

    override fun onKeyUp(keyCode: Int, event: KeyEvent?): Boolean {
        if (keyCode == KeyEvent.KEYCODE_BACK) {
            org.lwjgl.glfw.CallbackBridge.nativeSendKey(GLFW_KEY_ESCAPE, 0, GLFW_RELEASE, 0)
            return true
        }
        return super.onKeyUp(keyCode, event)
    }

    private companion object {
        // GLFW values (fixed by the GLFW API).
        const val GLFW_KEY_ESCAPE = 256
        const val GLFW_PRESS = 1
        const val GLFW_RELEASE = 0
    }
}
