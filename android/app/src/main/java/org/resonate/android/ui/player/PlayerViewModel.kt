package org.resonate.android.ui.player

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.flatMapLatest
import kotlinx.coroutines.flow.flow
import kotlinx.coroutines.flow.flowOf
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.withContext
import org.resonate.android.nativecore.HandedOff
import org.resonate.android.nativecore.SleepTimer
import org.resonate.android.nativecore.Snapshot
import org.resonate.android.nativecore.Track
import org.resonate.android.nativecore.Transport
import org.resonate.android.served
import org.resonate.android.ui.Repeat
import org.resonate.android.ui.Resonate
import org.resonate.android.ui.queueLanding

data class Playing(
    val track: Track?,
    val transport: Transport,
    val durationMs: Long,
    val volume: Float,
    val shuffle: Boolean,
    val repeat: Repeat,
    val sleep: SleepTimer?,
    val waiting: String?,
    val queueIndex: Int,
) {
    val active: Boolean
        get() = transport == Transport.PLAYING || transport == Transport.LOADING
}

data class QueueView(
    val history: List<Track>,
    val current: Track?,
    val upNext: List<Track>,
    val handoffs: List<HandedOff>,
) {
    val size: Int
        get() = history.size + (if (current == null) 0 else 1) + upNext.size

    val firstUpNext: Int
        get() = history.size + (if (current == null) 0 else 1)
}

private fun Snapshot.playing() =
    Playing(
        track = current,
        transport = transport,
        durationMs = durationMs.toLong(),
        volume = volume,
        shuffle = shuffle,
        repeat =
            when (repeat) {
                1u -> Repeat.ONE
                2u -> Repeat.ALL
                else -> Repeat.OFF
            },
        sleep = sleep,
        waiting = waiting,
        queueIndex = queueIndex.toInt(),
    )

@OptIn(ExperimentalCoroutinesApi::class)
class PlayerViewModel(private val resonate: Resonate) : ViewModel() {
    private val library = resonate.library

    val playing: StateFlow<Playing?> =
        library.snapshot
            .map { it?.playing() }
            .distinctUntilChanged()
            .stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), null)

    val position: StateFlow<Long> =
        library.place
            .flatMapLatest { place ->
                when {
                    place == null -> flowOf(0L)
                    !place.moving -> flowOf(place.positionMs)
                    else ->
                        flow {
                            while (true) {
                                emit(place.now())
                                delay(POSITION_EVERY_MS)
                            }
                        }
                }
            }
            .distinctUntilChanged()
            .stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), 0L)

    val sleepRemaining: StateFlow<Long?> =
        library.snapshot
            .map { it?.sleepRemainingMs?.toLong() }
            .distinctUntilChanged()
            .stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), null)

    val queue: StateFlow<QueueView> =
        combine(library.queue, playing, resonate.features.syncHandoffs) { rows, now, handoffs ->
                val currentRow = now?.track?.queueId
                val at = rows.indexOfFirst { currentRow != null && it.queueId == currentRow }
                if (at < 0) QueueView(emptyList(), null, rows, handoffs)
                else
                    QueueView(
                        rows.subList(0, at),
                        rows[at],
                        rows.subList(at + 1, rows.size),
                        handoffs,
                    )
            }
            .stateIn(
                viewModelScope,
                SharingStarted.WhileSubscribed(5_000),
                QueueView(emptyList(), null, emptyList(), emptyList()),
            )

    fun toggle() = resonate.playback.toggle()

    fun previous() = resonate.playback.previous()

    fun next() = resonate.playback.next()

    fun seek(positionMs: Long) = resonate.playback.seek(positionMs)

    fun shuffle(enabled: Boolean) = resonate.playback.shuffle(enabled)

    fun repeat(mode: Repeat) = resonate.playback.repeat(mode)

    fun volume(value: Float) = resonate.playback.volume(value)

    fun jump(index: Int) = resonate.playback.jump(index)

    fun move(from: Int, to: Int) {
        if (from == to) return
        val landing = queueLanding(from, to).toUInt()
        library.request { it.moveInQueue(from.toUInt(), landing) }
    }

    fun remove(track: Track) {
        val row = track.queueId ?: return
        library.request { it.removeFromQueue(row) }
    }

    fun removeAll(tracks: List<Track>) {
        val rows = tracks.mapNotNull { it.queueId }
        if (rows.isNotEmpty()) library.request { native -> rows.forEach { native.removeFromQueue(it) } }
    }

    fun clear() {
        library.request { it.clearQueue() }
    }

    fun sleep(timer: SleepTimer?) {
        library.request { it.setSleep(timer) }
    }

    fun continueFrom(handoff: HandedOff) {
        resonate.features.continueFrom(handoff) { resonate.playback.resume() }
    }

    fun saveAsPlaylist(name: String) {
        val rows = library.queue.value.filterNot { it.served }.map { it.id }
        resonate.attempt(viewModelScope) { native ->
            withContext(Dispatchers.IO) {
                val id = native.createPlaylist(name)
                if (rows.isNotEmpty()) native.addToPlaylist(id, rows)
            }
            resonate.features.loadPlaylists()
        }
    }
}

private const val POSITION_EVERY_MS = 50L
