package com.astral.aml

import android.content.Intent
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.MethodChannel

class MainActivity : FlutterActivity() {
    init {
        // Load the Rust cdylib here (before the Dart FFI opens it) so that
        // `JNI_OnLoad` runs in the main process and we can log/branch there.
        System.loadLibrary("rust_lib_aml")
    }

    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, CHANNEL).setMethodCallHandler { call, result ->
            when (call.method) {
                "startGame" -> {
                    val manifest = call.argument<String>("manifest")
                    if (manifest == null) {
                        result.error("BAD_ARGS", "missing manifest path", null)
                        return@setMethodCallHandler
                    }
                    // The manifest must stay in the app-private files area.
                    if (!manifest.startsWith("${filesDir.absolutePath}/")) {
                        result.error(
                            "UNSAFE_PATH",
                            "manifest path is outside the app-private directory",
                            null,
                        )
                        return@setMethodCallHandler
                    }
                    val intent = Intent(this, GameActivity::class.java)
                        .putExtra(EXTRA_MANIFEST, manifest)
                    startActivity(intent)
                    result.success(null)
                }
                "killGame" -> {
                    // Route the stop through the game-side service so it can
                    // remove its recents task before the process dies; a bare
                    // SIGKILL would leave a stale card behind. If no game is
                    // running the service starts, kills itself and exits.
                    val intent = Intent()
                        .setClassName(packageName, "com.astral.aml.game.GameService")
                        .setAction("com.astral.aml.game.KILL")
                    startService(intent)
                    result.success(null)
                }
                else -> result.notImplemented()
            }
        }
    }

    companion object {
        const val CHANNEL = "aml/launch"
        const val EXTRA_MANIFEST = "manifest"
    }
}
