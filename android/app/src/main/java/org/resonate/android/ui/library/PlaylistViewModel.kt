package org.resonate.android.ui.library

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn
import org.resonate.android.nativecore.MusicPlaylist
import org.resonate.android.nativecore.SavedSearch
import org.resonate.android.nativecore.SyncStanding
import org.resonate.android.nativecore.SyncedDevice
import org.resonate.android.nativecore.Track
import org.resonate.android.ui.Resonate

data class PlaylistSyncView(val kept: Boolean, val missing: UInt, val devices: List<SyncedDevice>)

fun reachableDevices(devices: List<SyncedDevice>): List<SyncedDevice> =
    devices.filter { !it.thisDevice && (it.online || it.nearby) }

class PlaylistViewModel(private val resonate: Resonate, private val id: ULong) : ViewModel() {
    private val features = resonate.features
    private val shared = SharingStarted.WhileSubscribed(5_000)

    val playlist: StateFlow<MusicPlaylist?> =
        features.playlists
            .map { all -> all.firstOrNull { it.id == id } }
            .stateIn(viewModelScope, shared, features.playlists.value.firstOrNull { it.id == id })

    val rows: StateFlow<Loaded<List<PlaylistEntry>>> =
        combine(features.openPlaylist, features.playlistRows, features.playlistLoading) { open, rows, loading ->
                if (open != id || loading) Loaded.Loading else Loaded.Ready(playlistEntries(rows))
            }
            .stateIn(viewModelScope, shared, Loaded.Loading)

    val sync: StateFlow<PlaylistSyncView?> =
        combine(features.playlistSync, features.syncStatus) { held, status ->
                if (status == null || status.standing == SyncStanding.OFF) null
                else
                    PlaylistSyncView(
                        kept = held?.takeIf { it.playlist == id }?.kept ?: false,
                        missing = held?.takeIf { it.playlist == id }?.missing ?: 0u,
                        devices = reachableDevices(status.devices),
                    )
            }
            .stateIn(viewModelScope, shared, null)

    val playing: StateFlow<ULong?> = resonate.playingId().stateIn(viewModelScope, shared, null)

    val catalog: StateFlow<List<Track>> = resonate.library.allTracks

    fun open() {
        if (features.openPlaylist.value != id) features.selectPlaylist(id)
        features.loadPlaylistSync(id)
    }

    private fun edit(revision: ULong?, change: (org.resonate.android.nativecore.MobilePlayer, ULong) -> Unit) {
        val at = revision ?: features.playlists.value.firstOrNull { it.id == id }?.revision ?: return
        features.editPlaylist { change(it, at) }
    }

    fun move(from: Int, to: Int, revision: ULong?) {
        if (from == to) return
        edit(revision) { native, at -> native.moveInPlaylist(id, at, from.toUInt(), to.toUInt()) }
    }

    fun remove(position: Int, revision: ULong?) =
        edit(revision) { native, at -> native.removeFromPlaylist(id, at, position.toUInt()) }

    fun rename(name: String) = edit(null) { native, at -> native.renamePlaylist(id, at, name) }

    fun revise(name: String, search: SavedSearch) =
        edit(null) { native, at -> native.reviseSearch(id, at, name, search) }

    fun delete() = edit(null) { native, at -> native.deletePlaylist(id, at) }

    fun undo() =
        features.editPlaylist { native ->
            if (!native.undoPlaylist()) resonate.library.notice("There is no playlist edit to undo")
        }

    fun undoDeletion() = features.editPlaylist { it.undoPlaylist() }

    fun add(tracks: List<Track>) {
        if (tracks.isNotEmpty()) features.addToPlaylist(id, tracks)
    }

    fun keep(kept: Boolean) = features.keepSongs(id, kept)

    fun fetch(device: SyncedDevice) = features.fetchMissing(id, device)
}
