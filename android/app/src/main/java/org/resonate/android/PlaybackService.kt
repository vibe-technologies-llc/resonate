package org.resonate.android

import android.app.PendingIntent
import android.content.Intent
import android.os.Bundle
import androidx.media3.common.util.UnstableApi
import androidx.media3.session.CommandButton
import androidx.media3.session.MediaLibraryService
import androidx.media3.session.MediaSession
import androidx.media3.session.SessionCommand
import androidx.media3.session.SessionError
import androidx.media3.session.SessionResult
import com.google.common.util.concurrent.Futures
import com.google.common.util.concurrent.ListenableFuture

private val closePlayback = SessionCommand("resonate.close", Bundle.EMPTY)

@UnstableApi
private class TrustedControllers(private val close: () -> Unit) :
    MediaLibraryService.MediaLibrarySession.Callback {
    override fun onConnect(
        session: MediaSession,
        controller: MediaSession.ControllerInfo,
    ): MediaSession.ConnectionResult =
        if (session.isMediaNotificationController(controller) || controller.isTrusted)
            MediaSession.ConnectionResult.AcceptedResultBuilder(session)
                .setAvailableSessionCommands(
                    MediaSession.ConnectionResult.DEFAULT_SESSION_AND_LIBRARY_COMMANDS.buildUpon()
                        .add(closePlayback)
                        .build()
                )
                .build()
        else MediaSession.ConnectionResult.reject()

    override fun onCustomCommand(
        session: MediaSession,
        controller: MediaSession.ControllerInfo,
        customCommand: SessionCommand,
        args: Bundle,
    ): ListenableFuture<SessionResult> {
        if (customCommand != closePlayback)
            return Futures.immediateFuture(SessionResult(SessionError.ERROR_NOT_SUPPORTED))
        close()
        return Futures.immediateFuture(SessionResult(SessionResult.RESULT_SUCCESS))
    }
}

@UnstableApi
class PlaybackService : MediaLibraryService() {
    private var session: MediaLibrarySession? = null
    private var player: RustMediaPlayer? = null

    override fun onCreate() {
        super.onCreate()
        setShowNotificationForIdlePlayer(SHOW_NOTIFICATION_FOR_IDLE_PLAYER_NEVER)
        val library = (application as ResonateApplication).library
        library.retain()
        val adapter = RustMediaPlayer(this, library)
        player = adapter
        val openApp =
            PendingIntent.getActivity(
                this,
                0,
                Intent(this, MainActivity::class.java)
                    .addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP),
                PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
            )
        session =
            MediaLibrarySession.Builder(this, adapter, TrustedControllers(::close))
                .setSessionActivity(openApp)
                .setMediaButtonPreferences(
                    listOf(
                        CommandButton.Builder(CommandButton.ICON_UNDEFINED)
                            .setCustomIconResId(R.drawable.ic_notification_close)
                            .setDisplayName(getString(R.string.notification_close))
                            .setSessionCommand(closePlayback)
                            .setSlots(
                                CommandButton.SLOT_FORWARD_SECONDARY,
                                CommandButton.SLOT_OVERFLOW,
                            )
                            .build()
                    )
                )
                .build()
    }

    private fun close() {
        player?.stop()
        stopSelf()
    }

    override fun onGetSession(controllerInfo: MediaSession.ControllerInfo): MediaLibrarySession? =
        session

    override fun onTaskRemoved(rootIntent: Intent?) {
        pauseAllPlayersAndStopSelf()
        (application as ResonateApplication).library.saveNow()
    }

    override fun onDestroy() {
        session?.release()
        player?.release()
        session = null
        super.onDestroy()
    }
}
