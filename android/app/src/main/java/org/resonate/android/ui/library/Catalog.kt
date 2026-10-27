package org.resonate.android.ui.library

import kotlin.random.Random
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.channelFlow
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.flowOf
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.withContext
import org.resonate.android.nativecore.MobilePlayer
import org.resonate.android.nativecore.Track
import org.resonate.android.ui.Resonate

sealed interface Loaded<out T> {
    data object Loading : Loaded<Nothing>

    data object Failed : Loaded<Nothing>

    data class Ready<T>(val value: T) : Loaded<T>
}

fun <T> Loaded<T>.valueOrNull(): T? = (this as? Loaded.Ready)?.value

private data class CatalogMark(val songs: List<Track>, val revision: ULong?, val artwork: Int)

fun Resonate.catalogChanges(): Flow<Any> =
    combine(
            library.allTracks,
            library.snapshot.map { it?.catalogRevision }.distinctUntilChanged(),
            features.artworkGeneration,
        ) { songs, revision, artwork ->
            CatalogMark(songs, revision, artwork)
        }
        .distinctUntilChanged { old, new ->
            old.songs === new.songs && old.revision == new.revision && old.artwork == new.artwork
        }

fun Resonate.playingId(): Flow<ULong?> =
    library.snapshot.map { it?.current?.id }.distinctUntilChanged()

fun Resonate.catalogReady(): Flow<Boolean> =
    combine(library.snapshot.map { it != null }, library.allTracks.map { it.isNotEmpty() }) {
            running,
            filled ->
            running || filled
        }
        .distinctUntilChanged()

class Retry {
    val asked = MutableStateFlow(0)

    fun again() {
        asked.value++
    }
}

fun <T> Resonate.catalogRead(
    retry: Retry,
    follow: Boolean = true,
    query: (MobilePlayer) -> T,
): Flow<Loaded<T>> = channelFlow {
    var shown: Loaded<T> = Loaded.Loading
    val changes = if (follow) catalogChanges() else flowOf(Unit)
    combine(changes, retry.asked) { _, asked -> asked }
        .collectLatest {
            if (shown is Loaded.Failed) {
                shown = Loaded.Loading
                send(shown)
            }
            val answer = read { native -> withContext(Dispatchers.IO) { query(native) } }
            shown =
                answer.fold(
                    onSuccess = { Loaded.Ready(it) },
                    onFailure = { if (shown is Loaded.Ready) shown else Loaded.Failed },
                )
            send(shown)
        }
}

fun Resonate.current(tracks: List<Track>): List<Track> =
    tracks.map { library.findTrack(it.id) ?: it }

fun Resonate.currentRows(tracks: Flow<Loaded<List<Track>>>): Flow<Loaded<List<Track>>> =
    combine(tracks, library.allTracks) { loaded, _ ->
        when (loaded) {
            is Loaded.Ready -> Loaded.Ready(current(loaded.value))
            else -> loaded
        }
    }

fun <T> pagedFully(page: (offset: UInt, limit: UInt) -> List<T>): List<T> {
    val rows = ArrayList<T>()
    do {
        val next = page(rows.size.toUInt(), PAGE_ROWS)
        rows.addAll(next)
    } while (next.size == PAGE_ROWS.toInt())
    return rows
}

fun Resonate.playInOrder(tracks: List<Track>, index: Int = 0) {
    if (tracks.isEmpty()) return
    playback.shuffle(false)
    playback.play(tracks, index)
}

fun Resonate.playShuffled(tracks: List<Track>) {
    if (tracks.isEmpty()) return
    playback.play(tracks, Random.nextInt(tracks.size))
    playback.shuffle(true)
}

fun Resonate.playNext(tracks: List<Track>) {
    val ids = tracks.map { it.id }
    if (ids.isNotEmpty()) library.request { it.playNext(ids) }
}

fun Resonate.addToQueue(tracks: List<Track>) {
    val ids = tracks.map { it.id }
    if (ids.isNotEmpty()) library.request { it.addToQueue(ids) }
}

private const val PAGE_ROWS = 500u
