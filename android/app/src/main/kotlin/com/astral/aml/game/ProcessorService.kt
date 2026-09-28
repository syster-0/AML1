package com.astral.aml.game

import android.app.Service
import android.content.Intent
import android.os.IBinder
import com.astral.aml.MainActivity
import kotlin.concurrent.thread

/**
 * Headless host for Forge/NeoForge install processors.
 *
 * Runs in the dedicated `:proc` process (separate from `:game`, so installs
 * also work while another instance is running). A HotSpot JVM can be created
 * only once per process and several processor mains call `System.exit`, so
 * one process runs exactly one processor; the LAUNCHER kills `:proc` once the
 * run's Exit frame arrives (the service never kills itself — a self-kill
 * races with the next start request, which AMS would then deliver to the
 * dying process with its JVM slot already spent).
 *
 * There is deliberately no UI: progress and logs travel over the manifest's
 * IPC socket to the launcher process.
 */
class ProcessorService : Service() {
    init {
        // Same cdylib as GameActivity; JNI_OnLoad takes the `:proc` branch.
        System.loadLibrary("rust_lib_aml")
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        // Defensive: a process that already ran a processor can never host
        // another JVM. Refuse a (re)delivered intent instead of booting a
        // doomed second VM in it.
        if (GameBridge.nativeIsFinished()) {
            stopSelf(startId)
            return START_NOT_STICKY
        }
        val manifest = intent?.getStringExtra(MainActivity.EXTRA_MANIFEST)
        // Same entry rule as GameActivity: manifest must be a private path.
        if (manifest == null || !manifest.startsWith("${filesDir.absolutePath}/")) {
            stopSelf(startId)
            return START_NOT_STICKY
        }

        thread(name = "aml-proc-watch") {
            // Boots the VM on a Rust-owned thread; returns immediately.
            GameBridge.nativeStart(manifest, filesDir.absolutePath)
            while (!GameBridge.nativeIsFinished()) {
                Thread.sleep(200)
            }
            // The launcher owns the process lifecycle (it kills :proc after
            // the Exit frame); just release the service.
            stopSelf(startId)
        }
        return START_NOT_STICKY
    }
}
