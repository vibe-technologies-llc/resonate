package org.resonate.android.ui.search

import androidx.compose.foundation.text.input.TextFieldState
import androidx.compose.foundation.text.input.setTextAndPlaceCursorAtEnd
import androidx.compose.runtime.snapshotFlow
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.FlowPreview
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.debounce
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.drop
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.resonate.android.nativecore.LibrarySearch
import org.resonate.android.nativecore.MobilePlayer
import org.resonate.android.nativecore.SavedSearch
import org.resonate.android.nativecore.SearchAlbum
import org.resonate.android.nativecore.SearchArtist
import org.resonate.android.nativecore.SearchRelease
import org.resonate.android.nativecore.SearchSong
import org.resonate.android.nativecore.Track
import org.resonate.android.nativecore.foldedText
import org.resonate.android.nativecore.highlights
import org.resonate.android.ui.Resonate

data class LibraryAnswer(
    val query: String,
    val found: LibrarySearch,
    val lit: Lit,
    val completions: List<String>,
)

sealed interface LibraryShown {
    data object Idle : LibraryShown

    data object Asking : LibraryShown

    data class Answered(val answer: LibraryAnswer, val asking: Boolean) : LibraryShown

    data class Failed(val query: String) : LibraryShown
}

sealed interface OnlineAnswer {
    data class Songs(val songs: List<SearchSong>) : OnlineAnswer

    data class Albums(val albums: List<SearchAlbum>, val next: UInt? = null) : OnlineAnswer

    data class Artists(val artists: List<SearchArtist>) : OnlineAnswer

    data class Editions(val releases: List<SearchRelease>) : OnlineAnswer

    data class Link(
        val name: String,
        val songs: List<SearchSong>,
        val releases: List<SearchRelease>,
        val artists: List<SearchArtist>,
    ) : OnlineAnswer
}

sealed interface OnlineSection<out T> {
    data object Loading : OnlineSection<Nothing>

    data class Failed(val reason: String?) : OnlineSection<Nothing>

    data class Ready<T>(val items: List<T>) : OnlineSection<T>
}

data class OnlineTop(
    val songs: OnlineSection<SearchSong>,
    val albums: OnlineSection<SearchAlbum>,
    val artists: OnlineSection<SearchArtist>,
)

sealed interface OnlineShown {
    data object Idle : OnlineShown

    data object Loading : OnlineShown

    data class Failed(val reason: String?) : OnlineShown

    data class Ready(val answer: OnlineAnswer, val more: Boolean) : OnlineShown
}

private sealed interface Fetched {
    data object Loading : Fetched

    data class Failed(val reason: String?) : Fetched

    data class Ready(val answer: OnlineAnswer) : Fetched
}

@OptIn(FlowPreview::class)
class SearchViewModel(private val resonate: Resonate) : ViewModel() {
    private val features = resonate.features
    private val library = resonate.library

    val field = TextFieldState()
    val downloads = Downloads(resonate, viewModelScope)

    private val query = MutableStateFlow("")
    private val mutableScope = MutableStateFlow(LibraryScope.TOP)
    private val answer = MutableStateFlow<LibraryAnswer?>(null)
    private val asking = MutableStateFlow(false)
    private val failedQuery = MutableStateFlow<String?>(null)
    private val mutableTrail = MutableStateFlow(OnlineTrail())
    private val fetched = MutableStateFlow<Map<OnlineRequest, Fetched>>(emptyMap())
    private val mutableMore = MutableStateFlow(false)
    private val mutableMoreSongs = MutableStateFlow(false)
    private var chain: Job? = null

    val loadingMoreSongs: StateFlow<Boolean> = mutableMoreSongs.asStateFlow()
    val scope: StateFlow<LibraryScope> = mutableScope.asStateFlow()
    val trail: StateFlow<OnlineTrail> = mutableTrail.asStateFlow()
    val typed: StateFlow<String> = query.asStateFlow()
    val recent: StateFlow<List<String>> = features.recentSearches
    val online: StateFlow<Boolean> = features.online

    val playing: StateFlow<ULong?> =
        library.snapshot
            .map { it?.current?.id }
            .distinctUntilChanged()
            .stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), null)

    val libraryShown: StateFlow<LibraryShown> =
        combine(query, answer, asking, failedQuery) { text, found, busy, failed ->
                when {
                    text.isEmpty() -> LibraryShown.Idle
                    failed == text -> LibraryShown.Failed(text)
                    found == null -> LibraryShown.Asking
                    else -> LibraryShown.Answered(found, busy)
                }
            }
            .stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), LibraryShown.Idle)

    val onlineTop: StateFlow<OnlineTop?> =
        combine(query, fetched, features.online) { text, answers, enabled ->
                if (!enabled || text.length < ONLINE_TEXT_AT_LEAST || isLink(text)) null
                else
                    OnlineTop(
                        section(answers[OnlineRequest.Songs(text)]) { (it as? OnlineAnswer.Songs)?.songs },
                        section(answers[OnlineRequest.Albums(text)]) { (it as? OnlineAnswer.Albums)?.albums },
                        section(answers[OnlineRequest.Artists(text)]) { (it as? OnlineAnswer.Artists)?.artists },
                    )
            }
            .stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), null)

    val onlineShown: StateFlow<OnlineShown> =
        combine(mutableTrail, query, fetched, mutableMore) { trail, text, answers, more ->
                when (val request = shows(trail.top, text)) {
                    null -> OnlineShown.Idle
                    else ->
                        when (val found = answers[request]) {
                            null,
                            Fetched.Loading -> OnlineShown.Loading
                            is Fetched.Failed -> OnlineShown.Failed(found.reason)
                            is Fetched.Ready -> OnlineShown.Ready(found.answer, more)
                        }
                }
            }
            .stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), OnlineShown.Idle)

    init {
        viewModelScope.launch {
            snapshotFlow { field.text.toString() }.collect { text -> typedText(text.trim()) }
        }
        viewModelScope.launch {
            combine(query.debounce(LIBRARY_SETTLE_MS), library.allTracks) { text, _ -> text }
                .collectLatest(::answerLibrary)
        }
        viewModelScope.launch {
            query.debounce(ONLINE_SETTLE_MS).distinctUntilChanged().collect { text -> root(text) }
        }
        viewModelScope.launch {
            features.online.drop(1).collect { enabled -> if (enabled) root(query.value) }
        }
        viewModelScope.launch {
            features.searchAsked.collect { text ->
                if (text == null) return@collect
                features.searchAsked.value = null
                search(text)
            }
        }
        viewModelScope.launch {
            features.downloading.drop(1).collect { running ->
                if (!running) forgetHeld()
            }
        }
    }

    private fun typedText(text: String) {
        if (text == query.value) return
        query.value = text
        if (text.isEmpty()) {
            answer.value = null
            asking.value = false
        }
    }

    private suspend fun answerLibrary(text: String) {
        if (text.isEmpty() || isLink(text)) {
            answer.value = null
            asking.value = false
            return
        }
        asking.value = true
        val found =
            resonate.read { native -> withContext(Dispatchers.IO) { native.searchLibrary(text, LIBRARY_SONGS_AT_MOST) } }
        found
            .onSuccess { result ->
                val shown =
                    withContext(Dispatchers.Default) {
                        LibraryAnswer(
                            text,
                            result,
                            lighted(text, result),
                            completed(text, result),
                        )
                    }
                failedQuery.value = null
                answer.value = shown
            }
            .onFailure { failedQuery.value = text }
        asking.value = false
    }

    fun moreSongs() {
        val current = answer.value ?: return
        if (current.found.tracks.size.toUInt() >= current.found.trackCount) return
        if (mutableMoreSongs.value) return
        mutableMoreSongs.value = true
        viewModelScope.launch {
            try {
                readMoreSongs(current)
            } finally {
                mutableMoreSongs.value = false
            }
        }
    }

    private suspend fun readMoreSongs(current: LibraryAnswer) {
        val from = current.found.tracks.size
        val page =
            resonate.read { native ->
                withContext(Dispatchers.IO) {
                    native.searchTracks(current.query, from.toUInt(), LIBRARY_SONGS_AT_MOST)
                }
            }

        page.onSuccess { tracks ->
            val latest = answer.value
            if (latest == null || latest.query != current.query || latest.found.tracks.size != from) return@onSuccess
            if (tracks.isEmpty()) return@onSuccess
            val grown = latest.found.copy(tracks = (latest.found.tracks + tracks).distinctBy { it.id })
            val shown = withContext(Dispatchers.Default) { lighted(latest.query, grown) }
            answer.value = latest.copy(found = grown, lit = shown)
        }
    }

    private fun lighted(text: String, result: LibrarySearch): Lit =
        try {
            lit(text, result) { query, value, column -> highlights(query, value, column) }
        } catch (failure: Exception) {
            Lit.none
        }

    private fun completed(text: String, result: LibrarySearch): List<String> =
        try {
            offeredCompletions(result.completions, text) { foldedText(it) }
        } catch (failure: Exception) {
            emptyList()
        }

    fun retryLibrary() {
        failedQuery.value = null
        viewModelScope.launch { answerLibrary(query.value) }
    }

    fun search(text: String) {
        field.setTextAndPlaceCursorAtEnd(text)
        typedText(text.trim())
        mutableTrail.value = mutableTrail.value.rooted(text.trim())
        keep()
    }

    fun clear() {
        field.setTextAndPlaceCursorAtEnd("")
        typedText("")
    }

    fun keep() {
        features.keepRecentSearches(keptSearch(recent.value, query.value))
    }

    fun saveSearch(name: String, search: SavedSearch) {
        keep()
        features.saveSearch(name, search)
    }

    fun forget(text: String) {
        features.keepRecentSearches(forgottenSearch(recent.value, text))
    }

    fun forgetAll() {
        features.keepRecentSearches(emptyList())
    }

    fun choose(scope: LibraryScope) {
        mutableScope.value = scope
    }

    fun turnOnline() {
        features.setOnline(true)
    }

    fun play(tracks: List<Track>, index: Int) {
        keep()
        resonate.playback.play(tracks, index)
    }

    fun open(view: OnlineView) {
        keep()
        mutableTrail.value = mutableTrail.value.opened(view)
        fetch()
    }

    fun back(): Boolean {
        if (!mutableTrail.value.nested) return false
        mutableTrail.value = mutableTrail.value.back()
        fetch()
        return true
    }

    fun retryOnline() {
        val top = mutableTrail.value.top
        if (top is OnlineView.Top) askTop(query.value, forced = true)
        else shows(top, query.value)?.let { issue(it, forced = true) }
    }

    fun more() {
        val view = mutableTrail.value.top as? OnlineView.Discography ?: return
        val request = OnlineRequest.Discography(view.artist)
        val shown = (fetched.value[request] as? Fetched.Ready)?.answer as? OnlineAnswer.Albums ?: return
        val from = shown.next ?: return
        if (mutableMore.value) return
        mutableMore.value = true
        viewModelScope.launch {
            resonate
                .read { native -> withContext(Dispatchers.IO) { native.artistAlbums(view.artist, from) } }
                .onSuccess { page ->
                    val current = (fetched.value[request] as? Fetched.Ready)?.answer as? OnlineAnswer.Albums
                    if (current != null) {
                        val joined = (current.albums + page.albums).distinctBy { it.id }
                        settle(request, Fetched.Ready(OnlineAnswer.Albums(joined, page.next)))
                    }
                }
                .onFailure { failure -> (failure as? Exception)?.let(library::report) }
            mutableMore.value = false
        }
    }

    fun save(song: SearchSong) {
        features.saveWanted(listOf(song))
    }

    fun import(name: String, songs: List<SearchSong>) {
        features.importPlaylist(name, songs)
    }

    fun saveAll(songs: List<SearchSong>) {
        features.saveWanted(songs)
    }

    fun submit() {
        keep()
        root(query.value)
    }

    private fun root(text: String) {
        mutableTrail.value = mutableTrail.value.rooted(text)
        if (!features.online.value) return
        val top = mutableTrail.value.top
        if (top is OnlineView.Top) askTop(text, forced = false)
        else shows(top, text)?.let { issue(it, forced = false) }
    }

    private fun askTop(text: String, forced: Boolean) {
        chain?.cancel()
        if (!features.online.value || text.length < ONLINE_TEXT_AT_LEAST || isLink(text)) return
        chain =
            viewModelScope.launch {
                listOf(
                        OnlineRequest.Songs(text),
                        OnlineRequest.Albums(text),
                        OnlineRequest.Artists(text),
                    )
                    .forEach { ask(it, forced) }
            }
    }

    private fun fetch() {
        val request = shows(mutableTrail.value.top, query.value) ?: return
        issue(request, forced = false)
    }

    private fun issue(request: OnlineRequest, forced: Boolean) {
        viewModelScope.launch { ask(request, forced) }
    }

    private suspend fun ask(request: OnlineRequest, forced: Boolean) {
        val known = fetched.value[request]
        if (known == Fetched.Loading) return
        if (!forced && known is Fetched.Ready) return
        settle(request, Fetched.Loading)
        try {
            val result = resonate.read { native -> withContext(Dispatchers.IO) { asked(native, request) } }
            settle(request, result.fold({ Fetched.Ready(it) }, { Fetched.Failed(it.message) }))
        } catch (cancelled: CancellationException) {
            fetched.value = fetched.value - request
            throw cancelled
        }
    }

    private fun <T> section(found: Fetched?, items: (OnlineAnswer) -> List<T>?): OnlineSection<T> =
        when (found) {
            null,
            Fetched.Loading -> OnlineSection.Loading
            is Fetched.Failed -> OnlineSection.Failed(found.reason)
            is Fetched.Ready -> OnlineSection.Ready(items(found.answer).orEmpty())
        }

    private fun settle(request: OnlineRequest, value: Fetched) {
        val kept = fetched.value - request
        val trimmed =
            if (kept.size < ANSWERS_KEPT) kept
            else kept.entries.drop(kept.size - ANSWERS_KEPT + 1).associate { it.toPair() }
        fetched.value = trimmed + (request to value)
    }

    private fun forgetHeld() {
        val stale =
            fetched.value.filterValues { found ->
                found is Fetched.Ready &&
                    (found.answer is OnlineAnswer.Songs || found.answer is OnlineAnswer.Link)
            }
        if (stale.isEmpty()) return
        fetched.value = fetched.value - stale.keys
        fetch()
        askTop(query.value, forced = false)
    }

    private fun asked(native: MobilePlayer, request: OnlineRequest): OnlineAnswer =
        when (request) {
            is OnlineRequest.Songs -> OnlineAnswer.Songs(native.searchOnline(request.text))
            is OnlineRequest.Albums -> OnlineAnswer.Albums(native.searchAlbumsOnline(request.text))
            is OnlineRequest.Artists -> OnlineAnswer.Artists(native.searchArtistsOnline(request.text))
            is OnlineRequest.Discography ->
                native.artistAlbums(request.artist, 0u).let { OnlineAnswer.Albums(it.albums, it.next) }
            is OnlineRequest.Editions -> OnlineAnswer.Editions(native.onlineReleases(request.group))
            is OnlineRequest.Release -> OnlineAnswer.Songs(native.releaseSongs(request.id))
            is OnlineRequest.Link ->
                native.openOnlineLink(request.text).let {
                    OnlineAnswer.Link(it.name, it.songs, it.releases, it.artists)
                }
        }

}

private const val LIBRARY_SETTLE_MS = 150L
private const val ONLINE_SETTLE_MS = 450L
private const val ANSWERS_KEPT = 48
