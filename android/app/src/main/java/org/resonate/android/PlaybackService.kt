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
import com.google.common.util.concurrent.SettableFuture
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeout

private val closePlayback = SessionCommand("resonate.close", Bundle.EMPTY)

private const val RESUMPTION_WAITS_AT_MOST_MS = 8_000L

@UnstableApi
private class TrustedControllers(
    private val close: () -> Unit,
    private val library: LibraryRepository,
    private val scope: CoroutineScope,
) : MediaLibraryService.MediaLibrarySession.Callback {
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

    override fun onPlaybackResumption(
        mediaSession: MediaSession,
        controller: MediaSession.ControllerInfo,
        isForPlayback: Boolean,
    ): ListenableFuture<MediaSession.MediaItemsWithStartPosition> {
        val resumed = SettableFuture.create<MediaSession.MediaItemsWithStartPosition>()
        scope.launch {
            try {
                val saved =
                    withTimeout(RESUMPTION_WAITS_AT_MOST_MS) {
                        library.native()
                        library.queue.first { it.isNotEmpty() }
                    }
                val place = library.snapshot.value
                resumed.set(
                    MediaSession.MediaItemsWithStartPosition(
                        saved.map { it.mediaItem() },
                        (place?.queueIndex?.toInt() ?: 0).coerceIn(saved.indices),
                        place?.positionMs?.toLong() ?: 0L,
                    )
                )
            } catch (cancelled: CancellationException) {
                resumed.cancel(false)
                throw cancelled
            } catch (failure: Exception) {
                resumed.setException(failure)
            }
        }
        return resumed
    }

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
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)

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
            MediaLibrarySession.Builder(this, adapter, TrustedControllers(::close, library, scope))
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
        scope.cancel()
        session?.release()
        player?.release()
        session = null
        super.onDestroy()
    }
}
