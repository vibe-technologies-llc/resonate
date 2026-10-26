package org.resonate.android

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.media.AudioFocusRequest
import android.media.AudioManager
import android.os.Looper
import androidx.core.content.ContextCompat
import androidx.media3.common.C
import androidx.media3.common.MediaItem
import androidx.media3.common.MediaMetadata
import androidx.media3.common.Player
import androidx.media3.common.SimpleBasePlayer
import androidx.media3.common.util.UnstableApi
import com.google.common.util.concurrent.Futures
import com.google.common.util.concurrent.ListenableFuture
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.distinctUntilChangedBy
import kotlinx.coroutines.launch
import org.resonate.android.nativecore.Snapshot
import org.resonate.android.nativecore.Track
import org.resonate.android.nativecore.Transport

@UnstableApi
class RustMediaPlayer(private val context: Context, private val library: LibraryRepository) :
    SimpleBasePlayer(Looper.getMainLooper()) {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private val audio = context.getSystemService(AudioManager::class.java)
    @Volatile private var snapshot: Snapshot? = null
    private var tracks = emptyList<Track>()
    private var artwork: ByteArray? = null
    private var artworkId: ULong? = null
    private var resumeAfterFocus = false
    private var playlist: BuiltPlaylist? = null
    private val coverVersion = MutableStateFlow(0)
    private val focus =
        AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN)
            .setAudioAttributes(
                android.media.AudioAttributes.Builder()
                    .setUsage(android.media.AudioAttributes.USAGE_MEDIA)
                    .setContentType(android.media.AudioAttributes.CONTENT_TYPE_MUSIC)
                    .build()
            )
            .setWillPauseWhenDucked(false)
            .setOnAudioFocusChangeListener { change ->
                when (change) {
                    AudioManager.AUDIOFOCUS_LOSS -> {
                        resumeAfterFocus = false
                        library.request { it.pause() }
                    }
                    AudioManager.AUDIOFOCUS_LOSS_TRANSIENT -> {
                        resumeAfterFocus = snapshot.playsOrLoads()
                        library.request { it.pause() }
                    }
                    AudioManager.AUDIOFOCUS_GAIN ->
                        if (resumeAfterFocus) {
                            resumeAfterFocus = false
                            library.request { it.play() }
                        }
                }
            }
            .build()
    private val noisy =
        object : BroadcastReceiver() {
            override fun onReceive(context: Context?, intent: Intent?) {
                if (intent?.action == AudioManager.ACTION_AUDIO_BECOMING_NOISY)
                    outputBecameNoisy()
            }
        }

    internal fun outputBecameNoisy() {
        resumeAfterFocus = false
        library.request { it.pause() }
    }

    init {
        ContextCompat.registerReceiver(
            context,
            noisy,
            IntentFilter(AudioManager.ACTION_AUDIO_BECOMING_NOISY),
            ContextCompat.RECEIVER_NOT_EXPORTED,
        )
        scope.launch {
            library.snapshot.collect { value ->
                val moved = value.stateKey() != snapshot.stateKey()
                val stoppedItself =
                    snapshot.playsOrLoads() && !value.playsOrLoads() && !resumeAfterFocus
                snapshot = value
                if (stoppedItself) audio.abandonAudioFocusRequest(focus)
                ResonateWidgets.publish(
                    context,
                    value,
                    artwork.takeIf { artworkId == value?.current?.id },
                )
                if (moved) invalidateState()
            }
        }
        scope.launch {
            library.queue.collect {
                tracks = it
                invalidateState()
            }
        }
        scope.launch {
            library.features.artChanges.collect { songs ->
                if (snapshot?.current?.id in songs) coverVersion.value++
            }
        }
        scope.launch {
            combine(
                    library.snapshot,
                    library.features.artworkGeneration,
                    coverVersion,
                ) { value, generation, version ->
                    Triple(value?.current, generation, version)
                }
                .distinctUntilChangedBy { (track, generation, version) ->
                    listOf(track?.id, track?.artRevision, generation, version)
                }
                .collectLatest { (track, _, _) ->
                    artwork = null
                    artworkId = null
                    invalidateState()
                    if (track != null) {
                        artwork = library.features.mediaArtwork(track)
                        artworkId = track.id
                        ResonateWidgets.publish(context, snapshot, artwork)
                        invalidateState()
                    }
                }
        }
    }

    private fun playlistOf(value: Snapshot?): List<MediaItemData> {
        val cover = artwork
        val coverIndex =
            if (cover == null) null
            else
                value?.queueIndex?.toInt()?.takeIf { index ->
                    val track = tracks.getOrNull(index)
                    track != null && track.id == value.current?.id && track.id == artworkId
                }
        playlist
            ?.takeIf { it.tracks === tracks && it.cover === cover && it.coverIndex == coverIndex }
            ?.let {
                return it.items
            }
        val items =
            tracks.mapIndexed { index, track ->
                MediaItemData.Builder((track.queueId ?: track.id).toString())
                    .setMediaItem(track.mediaItem(cover.takeIf { index == coverIndex }))
                    .setDurationUs(track.durationMs.toLong() * 1000)
                    .setIsSeekable(true)
                    .build()
            }
        playlist = BuiltPlaylist(tracks, cover, coverIndex, items)
        return items
    }

    override fun getState(): State {
        val value = snapshot
        val playlist = playlistOf(value)
        return State.Builder()
            .setAvailableCommands(
                Player.Commands.Builder()
                    .addAll(
                        Player.COMMAND_PLAY_PAUSE,
                        Player.COMMAND_PREPARE,
                        Player.COMMAND_STOP,
                        Player.COMMAND_SEEK_IN_CURRENT_MEDIA_ITEM,
                        Player.COMMAND_SEEK_TO_NEXT,
                        Player.COMMAND_SEEK_TO_PREVIOUS,
                        Player.COMMAND_SEEK_TO_MEDIA_ITEM,
                        Player.COMMAND_SEEK_TO_NEXT_MEDIA_ITEM,
                        Player.COMMAND_SEEK_TO_PREVIOUS_MEDIA_ITEM,
                        Player.COMMAND_GET_TIMELINE,
                        Player.COMMAND_GET_CURRENT_MEDIA_ITEM,
                        Player.COMMAND_GET_METADATA,
                        Player.COMMAND_SET_MEDIA_ITEM,
                        Player.COMMAND_CHANGE_MEDIA_ITEMS,
                        Player.COMMAND_SET_REPEAT_MODE,
                        Player.COMMAND_SET_SHUFFLE_MODE,
                        Player.COMMAND_SET_VOLUME,
                        Player.COMMAND_RELEASE,
                    )
                    .build()
            )
            .setPlaylist(playlist)
            .setCurrentMediaItemIndex(
                if (playlist.isEmpty()) C.INDEX_UNSET
                else (value?.queueIndex?.toInt() ?: 0).coerceIn(playlist.indices)
            )
            .setContentPositionMs { snapshot?.positionMs?.toLong() ?: 0L }
            .setPlayWhenReady(
                value.playsOrLoads(),
                Player.PLAY_WHEN_READY_CHANGE_REASON_USER_REQUEST,
            )
            .setPlaybackState(
                when {
                    playlist.isEmpty() -> Player.STATE_IDLE
                    value?.transport == Transport.LOADING -> Player.STATE_BUFFERING
                    value?.transport == Transport.STOPPED -> Player.STATE_IDLE
                    else -> Player.STATE_READY
                }
            )
            .setRepeatMode(value?.repeat?.toInt() ?: Player.REPEAT_MODE_OFF)
            .setShuffleModeEnabled(value?.shuffle ?: false)
            .setVolume(value?.volume ?: 1f)
            .build()
    }

    override fun handleSetPlayWhenReady(playWhenReady: Boolean): ListenableFuture<*> {
        if (playWhenReady) {
            if (audio.requestAudioFocus(focus) == AudioManager.AUDIOFOCUS_REQUEST_GRANTED)
                library.request { it.play() }
        } else {
            resumeAfterFocus = false
            library.request { it.pause() }
            audio.abandonAudioFocusRequest(focus)
        }
        return Futures.immediateVoidFuture()
    }

    override fun handleSetMediaItems(
        mediaItems: List<MediaItem>,
        startIndex: Int,
        startPositionMs: Long,
    ): ListenableFuture<*> {
        val (ids, start) = BrowseIds.playable(mediaItems.map { it.mediaId }, startIndex)
        if (ids.isEmpty()) return Futures.immediateVoidFuture()

        library.request {
            it.load(ids, start.toUInt(), false)
            if (startPositionMs > 0) it.seek(startPositionMs.toULong())
        }
        return Futures.immediateVoidFuture()
    }

    override fun handleAddMediaItems(
        index: Int,
        mediaItems: List<MediaItem>,
    ): ListenableFuture<*> {
        val ids = mediaItems.mapNotNull { BrowseIds.trackOf(it.mediaId) }
        val moves = QueueEdits.appendedThenMoved(tracks.size, index, ids.size)
        library.request { native ->
            native.addToQueue(ids)
            moves.forEach { native.moveInQueue(it.from.toUInt(), it.to.toUInt()) }
        }
        return Futures.immediateVoidFuture()
    }

    override fun handleMoveMediaItems(
        fromIndex: Int,
        toIndex: Int,
        newIndex: Int,
    ): ListenableFuture<*> {
        val moves = QueueEdits.rangeMoved(tracks.size, fromIndex, toIndex, newIndex)
        library.request { native ->
            moves.forEach { native.moveInQueue(it.from.toUInt(), it.to.toUInt()) }
        }
        return Futures.immediateVoidFuture()
    }

    override fun handleRemoveMediaItems(fromIndex: Int, toIndex: Int): ListenableFuture<*> {
        val rows = QueueEdits.rowsIn(tracks.map { it.queueId }, fromIndex, toIndex)
        library.request { native -> rows.forEach { native.removeFromQueue(it) } }
        return Futures.immediateVoidFuture()
    }

    override fun handleSeek(
        mediaItemIndex: Int,
        positionMs: Long,
        seekCommand: Int,
    ): ListenableFuture<*> {
        val playing = snapshot?.queueIndex?.toInt()
        val moves =
            seekCommand != Player.COMMAND_SEEK_IN_CURRENT_MEDIA_ITEM && mediaItemIndex != playing
        val position = positionMs.coerceAtLeast(0).toULong()

        library.request {
            if (moves) it.jumpTo(mediaItemIndex.coerceAtLeast(0).toUInt())
            it.seek(position)
        }
        return Futures.immediateVoidFuture()
    }

    override fun handleSetVolume(volume: Float, flags: Int): ListenableFuture<*> {
        library.request { it.setVolume(volume) }
        return Futures.immediateVoidFuture()
    }

    override fun handleSetShuffleModeEnabled(shuffleModeEnabled: Boolean): ListenableFuture<*> {
        library.request { it.setShuffle(shuffleModeEnabled) }
        return Futures.immediateVoidFuture()
    }

    override fun handleSetRepeatMode(repeatMode: Int): ListenableFuture<*> {
        library.request { it.setRepeat(repeatMode.toUInt()) }
        return Futures.immediateVoidFuture()
    }

    override fun handlePrepare(): ListenableFuture<*> = Futures.immediateVoidFuture()

    override fun handleStop(): ListenableFuture<*> {
        resumeAfterFocus = false
        library.request { it.stop() }
        audio.abandonAudioFocusRequest(focus)
        return Futures.immediateVoidFuture()
    }

    override fun handleRelease(): ListenableFuture<*> {
        scope.cancel()
        ResonateWidgets.stopped(context)
        context.unregisterReceiver(noisy)
        audio.abandonAudioFocusRequest(focus)
        library.release()
        return Futures.immediateVoidFuture()
    }
}

@UnstableApi
private class BuiltPlaylist(
    val tracks: List<Track>,
    val cover: ByteArray?,
    val coverIndex: Int?,
    val items: List<SimpleBasePlayer.MediaItemData>,
)

private data class StateKey(
    val transport: Transport,
    val queueIndex: UInt,
    val repeat: UInt,
    val shuffle: Boolean,
    val volume: Float,
    val current: ULong?,
)

private fun Snapshot?.stateKey() =
    this?.let {
        StateKey(it.transport, it.queueIndex, it.repeat, it.shuffle, it.volume, it.current?.id)
    }

private fun Snapshot?.playsOrLoads() =
    this?.transport == Transport.PLAYING || this?.transport == Transport.LOADING

fun Track.mediaItem(artwork: ByteArray? = null): MediaItem =
    MediaItem.Builder()
        .setMediaId(id.toString())
        .setUri(uri)
        .setMediaMetadata(
            MediaMetadata.Builder()
                .setTitle(title)
                .setArtist(artist)
                .setAlbumTitle(album)
                .setAlbumArtist(albumArtist)
                .setArtworkData(artwork, MediaMetadata.PICTURE_TYPE_FRONT_COVER)
                .setIsPlayable(true)
                .setIsBrowsable(false)
                .build()
        )
        .build()
