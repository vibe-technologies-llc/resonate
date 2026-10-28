package org.resonate.android.ui.library

import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.PlaylistAdd
import androidx.compose.material.icons.automirrored.rounded.QueueMusic
import androidx.compose.material.icons.automirrored.rounded.PlaylistPlay
import androidx.compose.material3.MaterialShapes
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.toShape
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.lifecycle.ViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn
import org.resonate.android.nativecore.Track
import org.resonate.android.ui.LocalResonate
import org.resonate.android.ui.Mix
import org.resonate.android.ui.Resonate
import org.resonate.android.ui.components.LocalSheets
import org.resonate.android.ui.components.PlaylistCover
import org.resonate.android.ui.components.SongSelectionBar
import org.resonate.android.ui.components.rememberSelection
import org.resonate.android.ui.components.TrackRow
import org.resonate.android.ui.resonateViewModel

data class MixPage(val tracks: List<Track>, val covers: List<ULong>)

class FavouritesViewModel(resonate: Resonate) : ViewModel() {
    private val shared = SharingStarted.WhileSubscribed(5_000)

    val tracks: StateFlow<Loaded<List<Track>>> =
        resonate.library.allTracks
            .map { songs -> Loaded.Ready(songs.filter { it.favourite }) }
            .stateIn(viewModelScope, shared, Loaded.Ready(resonate.library.allTracks.value.filter { it.favourite }))

    val playing: StateFlow<ULong?> = resonate.playingId().stateIn(viewModelScope, shared, null)
}

class MixViewModel(private val resonate: Resonate, mix: Mix) : ViewModel() {
    private val shared = SharingStarted.WhileSubscribed(5_000)

    val retry = Retry()

    val page: StateFlow<Loaded<MixPage>> =
        combine(
                resonate.catalogRead(retry, follow = false) { native ->
                    val filter = mix.filter()
                    MixPage(pagedFully { offset, limit -> native.filteredTracks(filter, offset, limit) }, native.picturedBy(filter))
                },
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
fun FavouritesScreen() {
    val model = resonateViewModel { FavouritesViewModel(it) }
    val loaded by model.tracks.collectAsStateWithLifecycle()
    val playing by model.playing.collectAsStateWithLifecycle()
    SongCollection(
        title = "Favourites",
        overline = "Playlist",
        loaded = loaded,
        onRetry = {},
        playing = playing,
        tag = "favourites",
        empty = "Tap the heart on any song to keep it here.",
    ) { side ->
        ShapeCover(side, MaterialShapes.Heart.toShape())
    }
}

@Composable
fun MixScreen(mix: Mix) {
    val model = resonateViewModel(key = "mix:${mix.name}") { MixViewModel(it, mix) }
    val loaded by model.page.collectAsStateWithLifecycle()
    val playing by model.playing.collectAsStateWithLifecycle()
    val covers = loaded.valueOrNull()?.covers.orEmpty()
    SongCollection(
        title = mix.title,
        overline = "Made for you",
        detail = mix.subtitle,
        loaded = loaded.map { it.tracks },
        onRetry = model.retry::again,
        playing = playing,
        tag = "mix",
        empty = "Nothing fits this mix yet. Listen a little and come back.",
    ) { side ->
        PlaylistCover(covers, side, shape = MaterialTheme.shapes.extraLarge)
    }
}

private fun <T, R> Loaded<T>.map(transform: (T) -> R): Loaded<R> =
    when (this) {
        Loaded.Loading -> Loaded.Loading
        Loaded.Failed -> Loaded.Failed
        is Loaded.Ready -> Loaded.Ready(transform(value))
    }

@Composable
private fun SongCollection(
    title: String,
    overline: String,
    loaded: Loaded<List<Track>>,
    onRetry: () -> Unit,
    playing: ULong?,
    tag: String,
    empty: String,
    detail: String? = null,
    cover: @Composable (androidx.compose.ui.unit.Dp) -> Unit,
) {
    val resonate = LocalResonate.current
    val sheets = LocalSheets.current
    val tracks = loaded.valueOrNull().orEmpty()
    val selection = rememberSelection<ULong>()
    LaunchedEffect(tracks) { selection.retain(tracks.map { it.id }) }
    CollectionPage(
        title,
        loaded,
        onRetry,
        tag = tag,
        selectionBar =
            if (!selection.active) null
            else {
                {
                    SongSelectionBar(
                        tracks = selection.picked(tracks) { it.id },
                        onSelectAll = { selection.selectAll(tracks.map { it.id }) },
                        onClose = selection::clear,
                    )
                }
            },
        actions = {
            OverflowMenu(
                listOf(
                    listOf(
                        MenuEntry.Action("Play next", Icons.AutoMirrored.Rounded.PlaylistPlay) { resonate.playNext(tracks) },
                        MenuEntry.Action("Add to queue", Icons.AutoMirrored.Rounded.QueueMusic) { resonate.addToQueue(tracks) },
                        MenuEntry.Action("Add to playlist", Icons.AutoMirrored.Rounded.PlaylistAdd) { sheets.addToPlaylist(tracks) },
                    )
                ),
                description = "$title options",
            )
        },
    ) { shown ->
        item("header") {
            CollectionHeader(
                title = title,
                overline = overline,
                summary = collectionSummary(tracks = shown),
                extra =
                    detail?.let { text ->
                        {
                            Text(
                                text,
                                style = MaterialTheme.typography.bodyMedium,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                                textAlign = TextAlign.Center,
                                modifier = Modifier.fillMaxWidth(),
                            )
                        }
                    },
                onPlay = { resonate.playInOrder(shown) },
                onShuffle = { resonate.playShuffled(shown) },
                playable = shown.isNotEmpty(),
                cover = cover,
            )
        }
        if (shown.isEmpty()) item("empty") { EmptyCollection(detail = empty) }
        shown.forEachIndexed { index, track ->
            item("row:$index:${track.id}") {
                TrackRow(
                    track,
                    onClick = { resonate.playback.play(shown, index) },
                    playing = track.id == playing,
                    onMore = { sheets.song(track) },
                    selection = selection.row(track.id),
                )
            }
        }
    }
}
