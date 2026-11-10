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
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

private const val NOTIFICATION = 3
private const val CHANNEL = "party"
private const val LEAVE_THE_PARTY = "leave-party"
private const val READ_EVERY_MS = 2_000L

class PartyService : Service() {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private val library
        get() = (application as ResonateApplication).library

    private val together
        get() = library.features.together

    private val notifications
        get() = getSystemService(NotificationManager::class.java)

    private var watching: Job? = null
    private var lineShown: String? = null

    override fun onCreate() {
        super.onCreate()
        library.retain()
        notifications.createNotificationChannel(
            NotificationChannel(
                CHANNEL,
                getString(R.string.party_notification_channel),
                NotificationManager.IMPORTANCE_LOW,
            )
        )
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        startForeground(
            NOTIFICATION,
            notification(),
            ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK,
        )
        if (intent?.action == LEAVE_THE_PARTY) together.leaveParty()
        if (watching?.isActive != true) watching = scope.launch { watch() }
        return START_NOT_STICKY
    }

    private suspend fun watch() {
        while (together.keepsTheServiceForeground) {
            val line = together.party.value?.let { TogetherWords.partyLine(library.words, it) }
            if (line != lineShown) notifications.notify(NOTIFICATION, notification())
            delay(READ_EVERY_MS)
            try {
                together.readParty()
            } catch (failure: IllegalStateException) {
                library.report(failure)
            }
        }
        stopSelf()
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onTaskRemoved(rootIntent: Intent?) {
        together.leaveParty()
    }

    override fun onDestroy() {
        scope.cancel()
        library.release()
        super.onDestroy()
    }

    private fun notification(): Notification {
        val party = together.party.value
        lineShown = party?.let { TogetherWords.partyLine(library.words, it) }
        val leave =
            PendingIntent.getService(
                this,
                NOTIFICATION,
                Intent(this, PartyService::class.java).setAction(LEAVE_THE_PARTY),
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
            )
        val open =
            PendingIntent.getActivity(
                this,
                NOTIFICATION,
                Intent(this, MainActivity::class.java),
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
            )
        return Notification.Builder(this, CHANNEL)
            .setSmallIcon(R.drawable.ic_launcher_monochrome)
            .setContentTitle(getString(R.string.party_notification_title))
            .setContentText(lineShown)
            .setContentIntent(open)
            .setOngoing(true)
            .addAction(
                Notification.Action.Builder(
                        null,
                        getString(
                            if (party?.hosting == true) R.string.party_notification_end
                            else R.string.party_notification_leave
                        ),
                        leave,
                    )
                    .build()
            )
            .build()
    }
}
