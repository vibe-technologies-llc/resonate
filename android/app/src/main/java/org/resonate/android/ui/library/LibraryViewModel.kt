package org.resonate.android.ui.library

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.flatMapLatest
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn
import org.resonate.android.nativecore.AlbumHit
import org.resonate.android.nativecore.ArtistHit
import org.resonate.android.nativecore.CollectionOrder
import org.resonate.android.nativecore.MusicPlaylist
import org.resonate.android.nativecore.Track
import org.resonate.android.nativecore.TrackFilter
import org.resonate.android.nativecore.defaultTrackFilter
import org.resonate.android.ui.Resonate

@OptIn(ExperimentalCoroutinesApi::class)
class LibraryViewModel(private val resonate: Resonate) : ViewModel() {
    private val library = resonate.library
    private val features = resonate.features
    private val shared = SharingStarted.WhileSubscribed(5_000)

    val albumsRetry = Retry()
    val artistsRetry = Retry()
    val albumOrder = MutableStateFlow(CollectionOrder.ALPHABETICAL)
    val artistOrder = MutableStateFlow(CollectionOrder.ALPHABETICAL)

    val ready: StateFlow<Boolean> = resonate.catalogReady().stateIn(viewModelScope, shared, false)

    val catalogEmpty: StateFlow<Boolean> =
        library.allTracks.map { it.isEmpty() }.stateIn(viewModelScope, shared, false)

    val filter: StateFlow<TrackFilter?> = library.filter

    val songs: StateFlow<List<Track>> =
        combine(library.filter, library.tracks, library.allTracks) { chosen, filtered, all ->
                if (chosen == null) all else filtered
            }
            .stateIn(viewModelScope, shared, library.allTracks.value)

    val playing: StateFlow<ULong?> = resonate.playingId().stateIn(viewModelScope, shared, null)

    val albums: StateFlow<Loaded<List<AlbumHit>>> =
        albumOrder
            .flatMapLatest { order -> resonate.catalogRead(albumsRetry) { it.albums(order) } }
            .stateIn(viewModelScope, shared, Loaded.Loading)

    val artists: StateFlow<Loaded<List<ArtistHit>>> =
        artistOrder
            .flatMapLatest { order -> resonate.catalogRead(artistsRetry) { it.artists(order) } }
            .stateIn(viewModelScope, shared, Loaded.Loading)

    val playlists: StateFlow<List<MusicPlaylist>> = features.playlists

    val favourites: StateFlow<Int> =
        library.allTracks.map { songs -> songs.count { it.favourite } }.stateIn(viewModelScope, shared, 0)

    val genres: StateFlow<List<String>> =
        library.allTracks
            .map { songs -> songs.map { it.genre.trim() }.filter { it.isNotEmpty() }.distinct().sortedBy { it.lowercase() } }
            .stateIn(viewModelScope, shared, emptyList())

    init {
        features.loadPlaylists()
    }

    fun refreshPlaylists() = features.loadPlaylists()

    fun applyFilter(chosen: TrackFilter) = library.setFilter(chosen.trimmed())

    fun clearFilter() = library.clearFilter()

    fun change(transform: (TrackFilter) -> TrackFilter) =
        applyFilter(transform(library.filter.value ?: defaultTrackFilter()))

    fun createPlaylist(name: String) = features.createPlaylist(name)

    fun rescan() = library.rescan()
}
