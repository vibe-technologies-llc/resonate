package org.resonate.android

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.IBinder
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch

class DownloadService : Service() {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private val library
        get() = (application as ResonateApplication).library

    private val notifications
        get() = getSystemService(NotificationManager::class.java)

    private var watching: Job? = null
    private var lastStart = 0

    override fun onCreate() {
        super.onCreate()
        library.retain()
        notifications.createNotificationChannel(
            NotificationChannel(
                "downloads",
                getString(R.string.transfer_notification_channel),
                NotificationManager.IMPORTANCE_LOW,
            )
        )
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        lastStart = startId
        startForeground(2, notification(), ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC)
        library.features.transferServiceRunning.value = true
        if (watching?.isActive != true) watching = scope.launch { watch() }
        library.features.downloadServiceReady()
        if (intent?.action == "cancel-download") library.features.cancelDownload()
        return START_NOT_STICKY
    }

    private suspend fun watch() {
        while (
            currentCoroutineContext().isActive &&
                (library.features.downloading.value || library.features.transferring.value)
        ) {
            notifications.notify(2, notification())
            delay(1000)
        }
        finish()
    }

    private fun finish() {
        library.features.transferServiceRunning.value = false
        stopSelfResult(lastStart)
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onTimeout(startId: Int, fgsType: Int) {
        library.features.cancelDownload()
        watching?.cancel()
        library.features.transferServiceRunning.value = false
        stopSelf()
    }

    override fun onDestroy() {
        library.features.transferServiceRunning.value = false
        scope.cancel()
        library.release()
        super.onDestroy()
    }

    private fun notification(): Notification {
        val cancel =
            PendingIntent.getService(
                this,
                2,
                Intent(this, DownloadService::class.java).setAction("cancel-download"),
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
            )
        val open =
            PendingIntent.getActivity(
                this,
                0,
                Intent(this, MainActivity::class.java),
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
            )
        val builder =
            Notification.Builder(this, "downloads")
                .setSmallIcon(android.R.drawable.stat_sys_download)
                .setContentIntent(open)
                .setOngoing(true)
                .setProgress(0, 0, true)
        if (!library.features.downloading.value) {
            val moving = library.features.syncStatus.value?.moving ?: 0u
            return builder
                .setContentTitle(getString(R.string.transfer_notification_sync_title))
                .setContentText(
                    if (moving > 0u)
                        resources.getQuantityString(
                            R.plurals.transfer_notification_moving,
                            moving.toInt(),
                            moving.toInt(),
                        )
                    else getString(R.string.transfer_notification_keeping)
                )
                .build()
        }
        return builder
            .setContentTitle(getString(R.string.transfer_notification_title))
            .setContentText(
                getString(
                    R.string.transfer_notification_progress,
                    library.features.downloadStatus.value.orEmpty(),
                    (library.features.downloadedBytes.value / 1024uL).toLong(),
                )
            )
            .addAction(
                Notification.Action.Builder(
                        null,
                        getString(R.string.transfer_notification_cancel),
                        cancel,
                    )
                    .build()
            )
            .build()
    }
}
