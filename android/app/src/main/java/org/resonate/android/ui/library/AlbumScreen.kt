package org.resonate.android.ui.library

import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.PlaylistAdd
import androidx.compose.material.icons.automirrored.rounded.QueueMusic
import androidx.compose.material.icons.rounded.Edit
import androidx.compose.material.icons.rounded.Person
import androidx.compose.material.icons.automirrored.rounded.PlaylistPlay
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.stateIn
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.resonate.android.nativecore.AlbumHit
import org.resonate.android.nativecore.Track
import org.resonate.android.ui.LocalResonate
import org.resonate.android.ui.Resonate
import org.resonate.android.ui.clock
import org.resonate.android.ui.components.LocalSheets
import org.resonate.android.ui.components.SectionHeader
import org.resonate.android.ui.components.SongArtwork
import org.resonate.android.ui.components.TrackRow
import org.resonate.android.ui.components.SongSelectionBar
import org.resonate.android.ui.components.rememberSelection
import org.resonate.android.ui.components.rememberArtwork
import org.resonate.android.ui.navigation.LocalNavigator
import org.resonate.android.ui.navigation.Screen
import org.resonate.android.ui.resonateViewModel
import org.resonate.android.ui.shownArtist
import org.resonate.android.ui.theme.ArtworkTheme
import org.resonate.android.ui.theme.rememberSeedOf

data class AlbumPage(val album: AlbumHit?, val tracks: List<Track>)

class AlbumViewModel(private val resonate: Resonate, key: String) : ViewModel() {
    private val shared = SharingStarted.WhileSubscribed(5_000)

    val retry = Retry()

    val page: StateFlow<Loaded<AlbumPage>> =
        combine(
                resonate.catalogRead(retry) { native -> AlbumPage(native.album(key), native.albumTracks(key)) },
                resonate.library.allTracks,
            ) { loaded, _ ->
                when (loaded) {
                    is Loaded.Ready -> Loaded.Ready(loaded.value.copy(tracks = resonate.current(loaded.value.tracks)))
                    else -> loaded
                }
            }
            .stateIn(viewModelScope, shared, Loaded.Loading)

    val playing: StateFlow<ULong?> = resonate.playingId().stateIn(viewModelScope, shared, null)
}

@Composable
fun AlbumScreen(screen: Screen.Album) {
    val model = resonateViewModel(key = "album:${screen.key}") { AlbumViewModel(it, screen.key) }
    val resonate = LocalResonate.current
    val navigator = LocalNavigator.current
    val sheets = LocalSheets.current
    val loaded by model.page.collectAsStateWithLifecycle()
    val playing by model.playing.collectAsStateWithLifecycle()
    val page = loaded.valueOrNull()
    val album = page?.album
    val title = album?.shownTitle ?: screen.title.ifBlank { "Unknown album" }
    val cover = album?.artworkTrack ?: screen.artworkTrack ?: page?.tracks?.firstOrNull()?.id
    val owner = album?.artist ?: screen.artist
    val various = album?.variousArtists == true
    val seed = rememberSeedOf(rememberArtwork(cover, 0u, SEED_EDGE))
    val songs = page?.tracks.orEmpty()
    val selection = rememberSelection<ULong>()
    LaunchedEffect(songs) { selection.retain(songs.map { it.id }) }
    ArtworkTheme(seed) {
        CollectionPage(
            title,
            loaded,
            model.retry::again,
            tag = "album",
        selectionBar =
            if (!selection.active) null
            else {
                {
                    SongSelectionBar(
                        tracks = selection.picked(songs) { it.id },
                        onSelectAll = { selection.selectAll(songs.map { it.id }) },
                        onClose = selection::clear,
                    )
                }
            },
            actions = {
                val tracks = page?.tracks.orEmpty()
                OverflowMenu(
                    listOf(
                        listOf(
                            MenuEntry.Action("Play next", Icons.AutoMirrored.Rounded.PlaylistPlay) { resonate.playNext(tracks) },
                            MenuEntry.Action("Add to queue", Icons.AutoMirrored.Rounded.QueueMusic) { resonate.addToQueue(tracks) },
                            MenuEntry.Action("Add to playlist", Icons.AutoMirrored.Rounded.PlaylistAdd) { sheets.addToPlaylist(tracks) },
                        ),
                        if (tracks.none(resonate.features.tags::writable)) emptyList()
                        else listOf(
                            MenuEntry.Action("Edit album tags", Icons.Rounded.Edit) {
                                sheets.editTags(tracks)
                            }
                        ),
                        if (various || owner.isBlank()) emptyList()
                        else listOf(
                            MenuEntry.Action("Go to artist", Icons.Rounded.Person) {
                                navigator.open(Screen.Artist(owner, cover))
                            }
                        ),
                    ),
                    description = "Album options",
                )
            },
        ) { shown ->
            val tracks = shown.tracks
            item("header") {
                CollectionHeader(
                    title = title,
                    overline = "Album",
                    summary = collectionSummary(shown.album?.year?.toString(), tracks = tracks),
                    onPlay = { resonate.playInOrder(tracks) },
                    onShuffle = { resonate.playShuffled(tracks) },
                    playable = tracks.isNotEmpty(),
                    link = {
                        if (various) ArtistLink("Various artists", null, null)
                        else if (owner.isNotBlank())
                            ArtistLink(owner, cover) { navigator.open(Screen.Artist(owner, cover)) }
                    },
                ) { side ->
                    SongArtwork(cover, side, shape = MaterialTheme.shapes.extraLarge)
                }
            }
            if (tracks.isEmpty()) item("empty") { EmptyCollection() }
            val discs = discsOf(tracks)
            discs.forEach { disc ->
                if (discs.size > 1) {
                    item("disc:${disc.number}") { SectionHeader(discTitle(disc.number)) }
                }
                disc.rows.forEach { (index, track) ->
                    item("row:$index:${track.id}") {
                        TrackRow(
                            track,
                            onClick = { resonate.playback.play(tracks, index) },
                            playing = track.id == playing,
                            number = trackNumber(track, index),
                            supporting = albumRowLine(track, owner, various),
                            onMore = { sheets.song(track) },
                            selection = selection.row(track.id),
                        )
                    }
                }
            }
        }
    }
}

private fun albumRowLine(track: Track, owner: String, various: Boolean): String =
    if (various || !track.artist.equals(owner, ignoreCase = true)) "${track.shownArtist} · ${clock(track.durationMs)}"
    else clock(track.durationMs)

private const val SEED_EDGE = 96
