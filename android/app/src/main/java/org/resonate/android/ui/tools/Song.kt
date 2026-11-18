package org.resonate.android.ui.tools

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.MusicOff
import androidx.compose.material.icons.rounded.Refresh
import androidx.compose.material.icons.rounded.SaveAlt
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.stateIn
import org.resonate.android.R
import org.resonate.android.nativecore.LyricsStanding
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
            stringResource(R.string.song_missing),
            detail = stringResource(R.string.song_missing_detail),
        )
    }
}

@Composable
fun LyricsScreen(track: ULong) {
    val song = rememberSong(track)
    val resonate = LocalResonate.current
    Page(
        title = song?.shownTitle ?: stringResource(R.string.song_lyrics),
        subtitle = song?.shownArtist,
        size = TitleSize.SMALL,
        actions = {
            IconButton(
                onClick = { resonate.features.loadLyrics(track, refresh = true) },
                enabled = song != null,
            ) {
                Icon(
                    Icons.Rounded.Refresh,
                    contentDescription = stringResource(R.string.song_refresh_lyrics),
                )
            }
            if (song != null) SaveLyricsMenu(song)
        },
    ) { padding ->
        if (song == null) MissingSong(padding)
        else
            Column(Modifier.fillMaxSize().padding(padding)) {
                LyricsView(
                    song,
                    MaterialTheme.typography.headlineMedium,
                    Modifier.weight(1f)
                        .fillMaxWidth()
                        .padding(horizontal = Sizes.gutter - 8.dp),
                )
                LyricTiming(song)
            }
    }
}

@Composable
private fun SaveLyricsMenu(song: Track) {
    val resonate = LocalResonate.current
    val tags = resonate.features.tags
    val sheet by resonate.features.lyrics.collectAsStateWithLifecycle()
    val beside = tags.savableBeside(song)
    val intoTags = tags.writable(song)
    val held = sheet?.takeIf { it.trackId == song.id && it.lines.isNotEmpty() }
    if (held == null || !(beside || intoTags)) return
    var open by remember { mutableStateOf(false) }
    Box {
        IconButton(onClick = { open = true }, modifier = Modifier.testTag("save-lyrics")) {
            Icon(Icons.Rounded.SaveAlt, contentDescription = stringResource(R.string.lyrics_save))
        }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            if (beside) {
                DropdownMenuItem(
                    text = { Text(stringResource(R.string.lyrics_save_beside)) },
                    onClick = {
                        open = false
                        tags.saveLyricsBeside(song, LyricsStanding.LEFT_ALONE)
                    },
                    modifier = Modifier.testTag("save-lyrics-beside"),
                )
                DropdownMenuItem(
                    text = { Text(stringResource(R.string.lyrics_save_over)) },
                    onClick = {
                        open = false
                        tags.saveLyricsBeside(song, LyricsStanding.REPLACED)
                    },
                    modifier = Modifier.testTag("save-lyrics-over"),
                )
            }
            if (intoTags) {
                DropdownMenuItem(
                    text = { Text(stringResource(R.string.lyrics_save_into_tags)) },
                    onClick = {
                        open = false
                        resonate.saveLyricsIntoTags(song)
                    },
                    modifier = Modifier.testTag("save-lyrics-into-tags"),
                )
            }
        }
    }
}
