package org.resonate.android.ui.home

import android.util.Log
import java.time.LocalDate
import java.util.TimeZone
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.async
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.resonate.android.LibraryRepository
import org.resonate.android.nativecore.AlbumHit
import org.resonate.android.nativecore.CollectionOrder
import org.resonate.android.nativecore.MobilePlayer
import org.resonate.android.nativecore.SuggestedList
import org.resonate.android.nativecore.defaultTrackFilter
import org.resonate.android.ui.onHome
import org.resonate.android.ui.suggestions

data class PlayedArtist(val name: String, val plays: ULong, val cover: ULong?)

private data class HomeShelves(val played: List<JumpBackTile.Album>, val recent: List<AlbumHit>)

private data class HomeListened(val heard: ListeningGlance.Heard, val artists: List<PlayedArtist>)

class HomeFeed(private val library: LibraryRepository, private val scope: CoroutineScope) {
    private val mutablePlayed = MutableStateFlow<List<JumpBackTile.Album>>(emptyList())
    val played: StateFlow<List<JumpBackTile.Album>> = mutablePlayed.asStateFlow()

    private val mutableRecent = MutableStateFlow<List<AlbumHit>>(emptyList())
    val recent: StateFlow<List<AlbumHit>> = mutableRecent.asStateFlow()

    private val mutableMixes = MutableStateFlow<List<SuggestedList>>(emptyList())
    val mixes: StateFlow<List<SuggestedList>> = mutableMixes.asStateFlow()

    private val mutableArtists = MutableStateFlow<List<PlayedArtist>>(emptyList())
    val artists: StateFlow<List<PlayedArtist>> = mutableArtists.asStateFlow()

    private val mutableHeard = MutableStateFlow<ListeningGlance.Heard?>(null)
    val heard: StateFlow<ListeningGlance.Heard?> = mutableHeard.asStateFlow()

    private val mutableDay = MutableStateFlow<LocalDate?>(null)
    val day: StateFlow<LocalDate?> = mutableDay.asStateFlow()

    private val lock = Any()
    private var running: Job? = null
    private var owed = false
    private var owedAfresh = false
    @Volatile private var moment: HomeMoment? = null

    suspend fun read(afresh: Boolean = false) {
        val (job, started) =
            synchronized(lock) {
                running?.let { reading ->
                    owed = true
                    owedAfresh = owedAfresh || afresh
                    return@synchronized reading to false
                }
                val made = scope.launch(start = CoroutineStart.LAZY) { readUntilSettled(afresh) }
                running = made
                made to true
            }
        if (started) job.start()
        job.join()
    }

    fun forget() {
        moment = null
    }

    fun ask(afresh: Boolean = false) {
        scope.launch { read(afresh) }
    }

    private suspend fun readUntilSettled(first: Boolean) {
        val self = currentCoroutineContext()[Job]
        var afresh = first
        try {
            while (true) {
                readOnce(afresh)
                afresh =
                    synchronized(lock) {
                        if (!owed) {
                            running = null
                            return
                        }
                        owed = false
                        owedAfresh.also { owedAfresh = false }
                    }
            }
        } finally {
            synchronized(lock) { if (running === self) running = null }
        }
    }

    private suspend fun readOnce(afresh: Boolean) {
        val now = part { native -> HomeMoment.of(native.catalogRevision()) } ?: return
        if (!afresh && now == moment) return

        val (shelves, mixes, listened) =
            coroutineScope {
                val shelves = async { part(::shelves) }
                val mixes = async { part { native -> native.suggestions().onHome() } }
                val listened = async { part(::listened) }
                Triple(shelves.await(), mixes.await(), listened.await())
            }

        if (shelves != null) {
            if (mutablePlayed.value != shelves.played) mutablePlayed.value = shelves.played
            if (mutableRecent.value != shelves.recent) mutableRecent.value = shelves.recent
        }
        if (mixes != null && mutableMixes.value != mixes) mutableMixes.value = mixes
        if (listened != null) {
            if (mutableHeard.value != listened.heard) mutableHeard.value = listened.heard
            if (mutableArtists.value != listened.artists) mutableArtists.value = listened.artists
            if (mutableDay.value != now.day) mutableDay.value = now.day
        }
        if (shelves != null && mixes != null && listened != null) moment = now
    }

    private suspend fun <T> part(read: (MobilePlayer) -> T): T? =
        try {
            withContext(Dispatchers.IO) { library.holding { native -> read(native) } }
        } catch (cancelled: CancellationException) {
            throw cancelled
        } catch (failure: Exception) {
            Log.w("Resonate", "A part of Home could not be read", failure)
            null
        }

    private fun shelves(native: MobilePlayer): HomeShelves {
        val heard =
            native.filteredTracks(JumpBackIn.heardLately(defaultTrackFilter()), 0u, JumpBackIn.SONGS_READ)
        val recent = native.albums(CollectionOrder.ADDED).take(SHELVED)
        return HomeShelves(JumpBackIn.albums(heard), recent)
    }

    private fun listened(native: MobilePlayer): HomeListened {
        val zone = TimeZone.getDefault()
        val offset = zone.getOffset(System.currentTimeMillis()) / MILLIS_PER_SECOND
        val statistics = native.statistics(0u)
        val detail = native.listeningDetail(0u, SHELVED.toUInt(), zone.id, offset)
        val heard =
            ListeningGlance.Heard(
                days = detail.days,
                firstHeard = detail.firstHeard,
                plays = statistics.plays,
                listenedMs = statistics.listenedMs,
                songsHeard = detail.songsHeard,
                songs = statistics.songs,
            )
        val artists =
            detail.artists.map { artist ->
                PlayedArtist(artist.name, artist.plays, native.mostPlayedBy(artist.name, 1u).firstOrNull()?.id)
            }
        return HomeListened(heard, artists)
    }

    companion object {
        const val SHELVED = 12
        private const val MILLIS_PER_SECOND = 1_000
    }
}
