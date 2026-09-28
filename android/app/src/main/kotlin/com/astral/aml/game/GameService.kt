package com.astral.aml.game

import android.app.ActivityManager
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import android.os.Process
import com.astral.aml.GameActivity
import com.astral.aml.R

/**
 * Foreground service keeping the `:game` process alive while a game runs.
 *
 * Without it the process drops to background priority as soon as the user
 * switches away and the system is free to kill the whole VM. The persistent
 * notification offers "return to game" (re-focus the activity) and a
 * force-stop action (kills the process; the launcher notices through the
 * IPC socket EOF and updates its state).
 *
 * Swiping the game card out of recents is treated as a close request:
 * `onTaskRemoved` kills the process instead of leaving a headless VM
 * running with no window (which the launcher would still show as running).
 */
class GameService : Service() {

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent?.action == ACTION_KILL) {
            killSelf()
            return START_NOT_STICKY
        }
        startForegroundWithNotification()
        // NOT_STICKY: a killed :game process means the game is dead — the
        // service must not respawn an empty process holding only a
        // notification.
        return START_NOT_STICKY
    }

    override fun onTaskRemoved(rootIntent: Intent?) {
        killSelf()
    }

    private fun startForegroundWithNotification() {
        val manager = getSystemService(NotificationManager::class.java)
        manager.createNotificationChannel(
            NotificationChannel(CHANNEL_ID, "游戏运行", NotificationManager.IMPORTANCE_LOW),
        )

        val returnIntent = PendingIntent.getActivity(
            this,
            0,
            Intent(this, GameActivity::class.java),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val killIntent = PendingIntent.getService(
            this,
            1,
            Intent(this, GameService::class.java).setAction(ACTION_KILL),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )

        val notification = Notification.Builder(this, CHANNEL_ID)
            .setSmallIcon(R.mipmap.ic_launcher)
            .setContentTitle("AML 正在运行游戏")
            .setContentText("切出后游戏保持运行；点按返回游戏")
            .setContentIntent(returnIntent)
            .addAction(
                Notification.Action.Builder(null, "强制结束", killIntent).build(),
            )
            .setOngoing(true)
            .build()

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            startForeground(NOTIFICATION_ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC)
        } else {
            startForeground(NOTIFICATION_ID, notification)
        }
    }

    private fun killSelf() {
        // Remove the game task so the recents card dies with the process; a
        // bare killProcess leaves a stale snapshot card behind. Only touch
        // the game's own task, never the launcher's.
        val am = getSystemService(ActivityManager::class.java)
        am.appTasks
            .filter {
                it.taskInfo?.baseActivity?.className == GameActivity::class.java.name ||
                    it.taskInfo?.topActivity?.className == GameActivity::class.java.name
            }
            .forEach { it.finishAndRemoveTask() }
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
        Process.killProcess(Process.myPid())
    }

    companion object {
        const val ACTION_KILL = "com.astral.aml.game.KILL"
        private const val CHANNEL_ID = "game_running"
        private const val NOTIFICATION_ID = 42
    }
}
