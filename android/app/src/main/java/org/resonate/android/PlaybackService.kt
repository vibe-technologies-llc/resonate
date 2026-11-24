package org.resonate.android

import android.app.PendingIntent
import android.content.Intent
import android.os.Bundle
import android.util.Log
import androidx.media3.common.Player
import androidx.media3.common.util.UnstableApi
import androidx.media3.session.CommandButton
import androidx.media3.session.MediaLibraryService
import androidx.media3.session.MediaSession
import androidx.media3.session.SessionCommand
import androidx.media3.session.SessionError
import androidx.media3.session.SessionResult
import androidx.media3.common.MediaItem
import androidx.media3.session.LibraryResult
import com.google.common.collect.ImmutableList
import com.google.common.util.concurrent.Futures
import com.google.common.util.concurrent.ListenableFuture
import com.google.common.util.concurrent.SettableFuture
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.TimeoutCancellationException
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.filterNotNull
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withTimeout

private val closePlayback = SessionCommand("resonate.close", Bundle.EMPTY)
private val toggleShuffle = SessionCommand("resonate.shuffle", Bundle.EMPTY)
private val cycleRepeat = SessionCommand("resonate.repeat", Bundle.EMPTY)

private fun nextRepeat(mode: Int): Int =
    when (mode) {
        Player.REPEAT_MODE_OFF -> Player.REPEAT_MODE_ALL
        Player.REPEAT_MODE_ALL -> Player.REPEAT_MODE_ONE
        else -> Player.REPEAT_MODE_OFF
    }

private const val RESUMPTION_WAITS_AT_MOST_MS = 8_000L

@UnstableApi
private suspend fun LibraryRepository.savedQueue(): MediaSession.MediaItemsWithStartPosition {
    val (saved, place) =
        withTimeout(RESUMPTION_WAITS_AT_MOST_MS) {
            ready()
            queueRead.first { it.tracks.isNotEmpty() } to snapshot.filterNotNull().first()
        }
    return MediaSession.MediaItemsWithStartPosition(
        saved.tracks.map { it.mediaItem() },
        QueueWindow.windowIndex(place.queueIndex.toInt(), saved),
        place.positionMs.toLong(),
    )
}

@UnstableApi
private class TrustedControllers(
    private val close: () -> Unit,
    private val library: LibraryRepository,
    private val browse: LibraryBrowse,
    private val scope: CoroutineScope,
    private val isBrowsingHost: (MediaSession.ControllerInfo) -> Boolean,
    private val carButtons: () -> List<CommandButton>,
) : MediaLibraryService.MediaLibrarySession.Callback {
    override fun onConnect(
        session: MediaSession,
        controller: MediaSession.ControllerInfo,
    ): MediaSession.ConnectionResult {
        val browsingHost = isBrowsingHost(controller)
        return if (session.isMediaNotificationController(controller) ||
            controller.isTrusted ||
            browsingHost)
            MediaSession.ConnectionResult.AcceptedResultBuilder(session)
                .setAvailableSessionCommands(
                    MediaSession.ConnectionResult.DEFAULT_SESSION_AND_LIBRARY_COMMANDS.buildUpon()
                        .add(closePlayback)
                        .add(toggleShuffle)
                        .add(cycleRepeat)
                        .build()
                )
                .apply { if (browsingHost) setMediaButtonPreferences(carButtons()) }
                .build()
        else MediaSession.ConnectionResult.reject()
    }

    override fun onGetLibraryRoot(
        session: MediaLibraryService.MediaLibrarySession,
        browser: MediaSession.ControllerInfo,
        params: MediaLibraryService.LibraryParams?,
    ): ListenableFuture<LibraryResult<MediaItem>> =
        answering { LibraryResult.ofItem(browse.root(), browse.rootParams(params)) }

    override fun onGetItem(
        session: MediaLibraryService.MediaLibrarySession,
        browser: MediaSession.ControllerInfo,
        mediaId: String,
    ): ListenableFuture<LibraryResult<MediaItem>> =
        answering {
            browse.item(mediaId)?.let { LibraryResult.ofItem(it, null) }
                ?: LibraryResult.ofError(SessionError.ERROR_BAD_VALUE)
        }

    override fun onGetChildren(
        session: MediaLibraryService.MediaLibrarySession,
        browser: MediaSession.ControllerInfo,
        parentId: String,
        page: Int,
        pageSize: Int,
        params: MediaLibraryService.LibraryParams?,
    ): ListenableFuture<LibraryResult<ImmutableList<MediaItem>>> =
        answering {
            browse.children(parentId, page, pageSize)?.let {
                LibraryResult.ofItemList(ImmutableList.copyOf(it), params)
            } ?: LibraryResult.ofError(SessionError.ERROR_BAD_VALUE)
        }

    override fun onSearch(
        session: MediaLibraryService.MediaLibrarySession,
        browser: MediaSession.ControllerInfo,
        query: String,
        params: MediaLibraryService.LibraryParams?,
    ): ListenableFuture<LibraryResult<Void>> =
        answering {
            session.notifySearchResultChanged(browser, query, browse.found(query).size, params)
            LibraryResult.ofVoid()
        }

    override fun onGetSearchResult(
        session: MediaLibraryService.MediaLibrarySession,
        browser: MediaSession.ControllerInfo,
        query: String,
        page: Int,
        pageSize: Int,
        params: MediaLibraryService.LibraryParams?,
    ): ListenableFuture<LibraryResult<ImmutableList<MediaItem>>> =
        answering {
            val found = browse.search(query).drop(page * pageSize).take(pageSize)
            LibraryResult.ofItemList(ImmutableList.copyOf(found), params)
        }

    override fun onSetMediaItems(
        mediaSession: MediaSession,
        controller: MediaSession.ControllerInfo,
        mediaItems: List<MediaItem>,
        startIndex: Int,
        startPositionMs: Long,
    ): ListenableFuture<MediaSession.MediaItemsWithStartPosition> {
        val expanded = SettableFuture.create<MediaSession.MediaItemsWithStartPosition>()
        scope.launch {
            try {
                expanded.set(
                    browse.expanded(mediaItems, startIndex, startPositionMs)
                        ?: MediaSession.MediaItemsWithStartPosition(
                            mediaItems,
                            startIndex,
                            startPositionMs,
                        )
                )
            } catch (cancelled: CancellationException) {
                expanded.cancel(false)
                throw cancelled
            } catch (failure: Exception) {
                expanded.setException(failure)
            }
        }
        return expanded
    }

    private fun <T : Any> answering(
        read: suspend () -> LibraryResult<T>
    ): ListenableFuture<LibraryResult<T>> {
        val answer = SettableFuture.create<LibraryResult<T>>()
        scope.launch {
            try {
                answer.set(read())
            } catch (cancelled: CancellationException) {
                answer.cancel(false)
                throw cancelled
            } catch (failure: Exception) {
                answer.set(LibraryResult.ofError(SessionError.ERROR_UNKNOWN))
            }
        }
        return answer
    }

    override fun onPlaybackResumption(
        mediaSession: MediaSession,
        controller: MediaSession.ControllerInfo,
        isForPlayback: Boolean,
    ): ListenableFuture<MediaSession.MediaItemsWithStartPosition> {
        val resumed = SettableFuture.create<MediaSession.MediaItemsWithStartPosition>()
        scope.launch {
            try {
                resumed.set(library.savedQueue())
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
        when (customCommand) {
            closePlayback -> close()
            toggleShuffle ->
                session.player.shuffleModeEnabled = !session.player.shuffleModeEnabled
            cycleRepeat -> session.player.repeatMode = nextRepeat(session.player.repeatMode)
            else ->
                return Futures.immediateFuture(SessionResult(SessionError.ERROR_NOT_SUPPORTED))
        }
        return Futures.immediateFuture(SessionResult(SessionResult.RESULT_SUCCESS))
    }
}

@UnstableApi
class PlaybackService : MediaLibraryService() {
    private var session: MediaLibrarySession? = null
    private var player: RustMediaPlayer? = null
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private val coldSkips = Mutex()
    private var resumedForASkip = false

    override fun onCreate() {
        super.onCreate()
        running = true
        setShowNotificationForIdlePlayer(SHOW_NOTIFICATION_FOR_IDLE_PLAYER_NEVER)
        val library = (application as ResonateApplication).library
        library.retain()
        val adapter = RustMediaPlayer(this, library)
        player = adapter
        val skipping =
            SkippingPlayer(
                adapter,
                previous = { library.request { it.previous() } },
                next = { library.request { it.next() } },
            )
        val openApp =
            PendingIntent.getActivity(
                this,
                0,
                Intent(this, MainActivity::class.java)
                    .addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP),
                PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
            )
        session =
            MediaLibrarySession.Builder(this, skipping, TrustedControllers(::close, library, library.browse, scope, ::isBrowsingHost) {
                    buttons(adapter, forCar = true)
                })
                .setSessionActivity(openApp)
                .setMediaButtonPreferences(buttons(adapter, forCar = false))
                .build()
        adapter.addListener(
            object : Player.Listener {
                override fun onShuffleModeEnabledChanged(shuffleModeEnabled: Boolean) =
                    refreshButtons(adapter)

                override fun onRepeatModeChanged(repeatMode: Int) = refreshButtons(adapter)
            }
        )
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        WidgetPresses.coldSkip(intent?.action)?.let(::skippedCold)
        return super.onStartCommand(intent, flags, startId)
    }

    private fun skippedCold(skip: ColdSkip) {
        val open = session ?: return
        addSession(open)
        val library = (application as ResonateApplication).library
        scope.launch {
            coldSkips.withLock {
                val player = open.player
                val holdsARow = player.currentMediaItem != null
                if (WidgetPresses.resumesBeforeSkipping(holdsARow, resumedForASkip)) {
                    resumedForASkip = true
                    resumed(player, library)
                }
                when (skip) {
                    ColdSkip.PREVIOUS -> player.seekToPrevious()
                    ColdSkip.NEXT -> player.seekToNext()
                }
                player.prepare()
                player.play()
            }
        }
    }

    private suspend fun resumed(player: Player, library: LibraryRepository) {
        try {
            val saved = library.savedQueue()
            player.setMediaItems(saved.mediaItems, saved.startIndex, saved.startPositionMs)
        } catch (late: TimeoutCancellationException) {
            Log.w("PlaybackService", "Resume the saved queue for a widget skip", late)
        } catch (cancelled: CancellationException) {
            throw cancelled
        } catch (failure: Exception) {
            Log.w("PlaybackService", "Resume the saved queue for a widget skip", failure)
        }
    }

    override fun onUpdateNotification(session: MediaSession, startInForegroundRequired: Boolean) {
        val holdsAParty = (application as ResonateApplication).library.features.together.keepsTheServiceForeground
        super.onUpdateNotification(session, startInForegroundRequired || holdsAParty)
    }

    private fun isBrowsingHost(controller: MediaSession.ControllerInfo): Boolean =
        packageManager.isBrowsingHost(controller.uid, controller.packageName)

    private fun refreshButtons(player: Player) {
        val open = session ?: return
        open.setMediaButtonPreferences(buttons(player, forCar = false))
        open.connectedControllers
            .filter(::isBrowsingHost)
            .forEach { open.setMediaButtonPreferences(it, buttons(player, forCar = true)) }
    }

    private fun buttons(player: Player, forCar: Boolean): List<CommandButton> =
        listOfNotNull(
            CommandButton.Builder(
                    if (player.shuffleModeEnabled) CommandButton.ICON_SHUFFLE_ON
                    else CommandButton.ICON_SHUFFLE_OFF
                )
                .setDisplayName(getString(R.string.notification_shuffle))
                .setSessionCommand(toggleShuffle)
                .setSlots(CommandButton.SLOT_OVERFLOW)
                .build(),
            if (forCar)
                CommandButton.Builder(
                        when (player.repeatMode) {
                            Player.REPEAT_MODE_ALL -> CommandButton.ICON_REPEAT_ALL
                            Player.REPEAT_MODE_ONE -> CommandButton.ICON_REPEAT_ONE
                            else -> CommandButton.ICON_REPEAT_OFF
                        }
                    )
                    .setDisplayName(getString(R.string.notification_repeat))
                    .setSessionCommand(cycleRepeat)
                    .setSlots(CommandButton.SLOT_OVERFLOW)
                    .build()
            else null,
            CommandButton.Builder(CommandButton.ICON_UNDEFINED)
                .setCustomIconResId(R.drawable.ic_notification_close)
                .setDisplayName(getString(R.string.notification_close))
                .setSessionCommand(closePlayback)
                .setSlots(CommandButton.SLOT_FORWARD_SECONDARY, CommandButton.SLOT_OVERFLOW)
                .build(),
        )

    private fun close() {
        player?.stopEveryZone()
        player?.stop()
        stopSelf()
    }

    companion object {
        @Volatile
        var running = false
            private set
    }

    override fun onGetSession(controllerInfo: MediaSession.ControllerInfo): MediaLibrarySession? =
        session

    override fun onTaskRemoved(rootIntent: Intent?) {
        player?.pauseSoundingZones()
        pauseAllPlayersAndStopSelf()
        (application as ResonateApplication).library.saveNow()
    }

    override fun onDestroy() {
        running = false
        scope.cancel()
        session?.release()
        player?.release()
        session = null
        super.onDestroy()
    }
}
