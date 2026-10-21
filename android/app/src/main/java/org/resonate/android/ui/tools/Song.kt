package org.resonate.android.ui.tools

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.MusicOff
import androidx.compose.material.icons.rounded.Refresh
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.stateIn
import org.resonate.android.nativecore.Track
import org.resonate.android.ui.LocalResonate
import org.resonate.android.ui.Resonate
import org.resonate.android.ui.components.EmptyState
import org.resonate.android.ui.components.Page
import org.resonate.android.ui.components.Sizes
import org.resonate.android.ui.components.TitleSize
import org.resonate.android.ui.resonateViewModel
import org.resonate.android.ui.shownArtist
import org.resonate.android.ui.shownTitle

class SongViewModel(resonate: Resonate, id: ULong) : ViewModel() {
    private val library = resonate.library

    val track: StateFlow<Track?> =
        combine(library.tracks, library.queue, library.snapshot) { _, _, _ -> library.findTrack(id) }
            .distinctUntilChanged()
            .stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), library.findTrack(id))
}

@Composable
fun rememberSong(id: ULong): Track? {
    val model = resonateViewModel(key = "song-$id") { SongViewModel(it, id) }
    val track by model.track.collectAsStateWithLifecycle()
    return track
}

@Composable
fun MissingSong(padding: PaddingValues) {
    Box(Modifier.fillMaxSize().padding(padding)) {
        EmptyState(
            Icons.Rounded.MusicOff,
            "This song is not in the library",
            detail = "It may have been removed or its folder is no longer shared with Resonate.",
        )
    }
}

@Composable
fun LyricsScreen(track: ULong) {
    val song = rememberSong(track)
    val resonate = LocalResonate.current
    Page(
        title = song?.shownTitle ?: "Lyrics",
        subtitle = song?.shownArtist,
        size = TitleSize.SMALL,
        actions = {
            IconButton(onClick = { resonate.features.loadLyrics(track, refresh = true) }, enabled = song != null) {
                Icon(Icons.Rounded.Refresh, contentDescription = "Refresh lyrics")
            }
        },
    ) { padding ->
        if (song == null) MissingSong(padding)
        else
            LyricsView(
                song,
                MaterialTheme.typography.headlineMedium,
                Modifier.fillMaxSize().padding(padding).padding(horizontal = Sizes.gutter - 8.dp),
            )
    }
}
