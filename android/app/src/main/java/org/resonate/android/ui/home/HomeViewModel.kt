package org.resonate.android.ui.home

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.conflate
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.flowOn
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.resonate.android.nativecore.AlbumHit
import org.resonate.android.nativecore.HandedOff
import org.resonate.android.nativecore.MusicPlaylist
import org.resonate.android.nativecore.SuggestedList
import org.resonate.android.nativecore.SyncedDevice
import org.resonate.android.nativecore.Track
import org.resonate.android.ui.Resonate
import org.resonate.android.ui.library.catalogChanges
import org.resonate.android.ui.library.catalogReady
import org.resonate.android.ui.library.playInOrder
import org.resonate.android.ui.library.savedSearchFully
import org.resonate.android.ui.settings.mayDrive
import org.resonate.android.ui.settings.orderedDevices

class HomeViewModel(private val resonate: Resonate) : ViewModel() {
    private val features = resonate.features
    private val shared = SharingStarted.WhileSubscribed(5_000)

    val feed = features.home

    val ready: StateFlow<Boolean> = resonate.catalogReady().stateIn(viewModelScope, shared, false)

    val songs: StateFlow<List<Track>> = resonate.library.allTracks

    val unread: StateFlow<Boolean> = resonate.library.unread

    fun reread() = resonate.library.reread()

    val anyFavourite: StateFlow<Boolean> =
        resonate.library.allTracks
            .map { songs -> songs.any { it.favourite } }
            .distinctUntilChanged()
            .flowOn(Dispatchers.Default)
            .stateIn(viewModelScope, shared, false)

    val played: StateFlow<List<JumpBackTile.Album>> = feed.played

    val recent: StateFlow<List<AlbumHit>> = feed.recent

    val mixes: StateFlow<List<SuggestedList>> = feed.mixes

    val artists: StateFlow<List<PlayedArtist>> = feed.artists

    val heard: StateFlow<ListeningGlance.Heard?> = feed.heard

    val playlists: StateFlow<List<MusicPlaylist>> = features.playlists

    val devices: StateFlow<List<SyncedDevice>> =
        features.syncStatus
            .map { status -> orderedDevices(status?.devices.orEmpty()).filter(::mayDrive) }
            .distinctUntilChanged()
            .stateIn(viewModelScope, shared, emptyList())

    val handoffs: StateFlow<List<HandedOff>> = features.syncHandoffs

    private val mutableRefreshing = MutableStateFlow(false)
    val refreshing: StateFlow<Boolean> = mutableRefreshing.asStateFlow()

    suspend fun followCatalog() {
        resonate.catalogChanges().conflate().collect { feed.read() }
    }

    fun cameForward() {
        features.loadPlaylists()
        feed.ask()
    }

    fun pull() {
        if (mutableRefreshing.value) return
        mutableRefreshing.value = true
        viewModelScope.launch {
            try {
                resonate.library.rescan()
                features.loadPlaylists()
                feed.read(afresh = true)
            } finally {
                mutableRefreshing.value = false
            }
        }
    }

    fun playAlbum(album: AlbumHit) =
        resonate.attempt(viewModelScope) { native ->
            val tracks = withContext(Dispatchers.IO) { native.albumTracks(album.key) }
            resonate.playInOrder(tracks)
        }

    fun playMix(list: SuggestedList) =
        resonate.attempt(viewModelScope) { native ->
            val tracks = withContext(Dispatchers.IO) { savedSearchFully(native, list.search) }
            resonate.playInOrder(tracks)
        }

    fun continueFrom(handoff: HandedOff) = features.continueFrom(handoff) { resonate.playback.resume() }

    fun createPlaylist(name: String) = features.createPlaylist(name)
}
