package org.resonate.android.ui.home

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.flowOn
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn
import org.resonate.android.nativecore.AlbumHit
import org.resonate.android.nativecore.ArtistHit
import org.resonate.android.nativecore.CollectionOrder
import org.resonate.android.nativecore.LibraryStatistics
import org.resonate.android.nativecore.MusicPlaylist
import org.resonate.android.nativecore.Track
import org.resonate.android.ui.Mix
import org.resonate.android.ui.Resonate
import org.resonate.android.ui.library.Loaded
import org.resonate.android.ui.library.Retry
import org.resonate.android.ui.library.catalogRead
import org.resonate.android.ui.library.catalogReady

data class MixShelf(val mix: Mix, val covers: List<ULong>)

class HomeViewModel(private val resonate: Resonate) : ViewModel() {
    private val features = resonate.features
    private val shared = SharingStarted.WhileSubscribed(5_000)

    val retry = Retry()

    val ready: StateFlow<Boolean> = resonate.catalogReady().stateIn(viewModelScope, shared, false)

    val songs: StateFlow<List<Track>> = resonate.library.allTracks

    val favourites: StateFlow<List<Track>> =
        resonate.library.allTracks
            .map { songs -> songs.filter { it.favourite } }
            .flowOn(Dispatchers.Default)
            .stateIn(viewModelScope, shared, emptyList())

    val recentAlbums: StateFlow<Loaded<List<AlbumHit>>> =
        resonate
            .catalogRead(retry) { it.albums(CollectionOrder.ADDED).take(SHELF_AT_MOST) }
            .stateIn(viewModelScope, shared, Loaded.Loading)

    val artists: StateFlow<Loaded<List<ArtistHit>>> =
        resonate
            .catalogRead(retry) { it.artists(CollectionOrder.PLAYS).take(SHELF_AT_MOST) }
            .stateIn(viewModelScope, shared, Loaded.Loading)

    val mixes: StateFlow<Loaded<List<MixShelf>>> =
        resonate
            .catalogRead(retry) { native ->
                Mix.entries.mapNotNull { mix ->
                    val filter = mix.filter()
                    val any = native.filteredTracks(filter, 0u, 1u).isNotEmpty()
                    if (any) MixShelf(mix, native.picturedBy(filter)) else null
                }
            }
            .stateIn(viewModelScope, shared, Loaded.Loading)

    val playlists: StateFlow<List<MusicPlaylist>> = features.playlists

    val statistics: StateFlow<LibraryStatistics?> = features.statistics

    fun refresh() {
        features.loadPlaylists()
        features.loadStatistics()
    }

    fun createPlaylist(name: String) = features.createPlaylist(name)
}

private const val SHELF_AT_MOST = 12
