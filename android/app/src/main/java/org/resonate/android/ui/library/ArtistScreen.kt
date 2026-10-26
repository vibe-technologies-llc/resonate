package org.resonate.android.ui.library

import android.net.Uri
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.PlaylistAdd
import androidx.compose.material.icons.automirrored.rounded.QueueMusic
import androidx.compose.material.icons.rounded.Link
import androidx.compose.material.icons.rounded.Person
import androidx.compose.material.icons.automirrored.rounded.PlaylistPlay
import androidx.compose.material3.Icon
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.resonate.android.nativecore.AlbumHit
import org.resonate.android.nativecore.ArtistInformation
import org.resonate.android.nativecore.Track
import org.resonate.android.ui.LocalResonate
import org.resonate.android.ui.Resonate
import org.resonate.android.ui.components.ArtistPortrait
import org.resonate.android.ui.components.LocalSheets
import org.resonate.android.ui.components.Picture
import org.resonate.android.ui.components.SectionHeader
import org.resonate.android.ui.components.TrackRow
import org.resonate.android.ui.components.rememberArtwork
import org.resonate.android.ui.components.rememberPicture
import org.resonate.android.ui.counted
import org.resonate.android.ui.navigation.LocalNavigator
import org.resonate.android.ui.navigation.Screen
import org.resonate.android.ui.resonateViewModel
import org.resonate.android.ui.theme.ArtworkTheme
import org.resonate.android.ui.theme.rememberSeedOf

data class ArtistPage(val albums: List<AlbumHit>, val tracks: List<Track>)

class ArtistViewModel(private val resonate: Resonate, name: String) : ViewModel() {
    private val shared = SharingStarted.WhileSubscribed(5_000)

    val retry = Retry()

    val page: StateFlow<Loaded<ArtistPage>> =
        combine(
                resonate.catalogRead(retry) { native -> ArtistPage(native.albumsBy(name), native.artistTracks(name)) },
                resonate.library.allTracks,
            ) { loaded, _ ->
                when (loaded) {
                    is Loaded.Ready -> Loaded.Ready(loaded.value.copy(tracks = resonate.current(loaded.value.tracks)))
                    else -> loaded
                }
            }
            .stateIn(viewModelScope, shared, Loaded.Loading)

    private val learned = MutableStateFlow<ArtistInformation?>(null)
    private var learnedFor: ULong? = null

    val information: StateFlow<ArtistInformation?> = learned.asStateFlow()

    val playing: StateFlow<ULong?> = resonate.playingId().stateIn(viewModelScope, shared, null)

    fun learn(song: ULong) {
        if (song == learnedFor) return
        learnedFor = song
        viewModelScope.launch {
            resonate
                .read { native -> withContext(Dispatchers.IO) { native.artistInformation(song) } }
                .onSuccess { found -> if (song == learnedFor) learned.value = found }
        }
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
fun ArtistScreen(screen: Screen.Artist) {
    val model = resonateViewModel(key = "artist:${screen.name}") { ArtistViewModel(it, screen.name) }
    val resonate = LocalResonate.current
    val navigator = LocalNavigator.current
    val sheets = LocalSheets.current
    val loaded by model.page.collectAsStateWithLifecycle()
    val information by model.information.collectAsStateWithLifecycle()
    val playing by model.playing.collectAsStateWithLifecycle()
    var everySong by rememberSaveable { mutableStateOf(false) }
    val page = loaded.valueOrNull()
    val portraitSong = screen.artworkTrack ?: page?.tracks?.firstOrNull()?.id
    val name = screen.name.ifBlank { "Unknown artist" }
    val picture = rememberPicture(information?.picture)
    val seed = rememberSeedOf(picture ?: rememberArtwork(portraitSong, 0u, 96))
    LaunchedEffect(portraitSong) { portraitSong?.let(model::learn) }
    ArtworkTheme(seed) {
        CollectionPage(
            name,
            loaded,
            model.retry::again,
            tag = "artist",
            actions = {
                val tracks = page?.tracks.orEmpty()
                OverflowMenu(
                    listOf(
                        listOf(
                            MenuEntry.Action("Play next", Icons.AutoMirrored.Rounded.PlaylistPlay) { resonate.playNext(tracks) },
                            MenuEntry.Action("Add to queue", Icons.AutoMirrored.Rounded.QueueMusic) { resonate.addToQueue(tracks) },
                            MenuEntry.Action("Add to playlist", Icons.AutoMirrored.Rounded.PlaylistAdd) { sheets.addToPlaylist(tracks) },
                        )
                    ),
                    description = "Artist options",
                )
            },
        ) { shown ->
            val tracks = shown.tracks
            val about = information
            item("header") {
                CollectionHeader(
                    title = name,
                    overline = about?.let { artistLine(it.kind, it.country, it.area) }?.ifBlank { null } ?: "Artist",
                    summary =
                        collectionSummary(
                            shown.albums.size.takeIf { it > 0 }?.let { counted(it, "album") },
                            tracks = tracks,
                        ),
                    onPlay = { resonate.playInOrder(tracks) },
                    onShuffle = { resonate.playShuffled(tracks) },
                    playable = tracks.isNotEmpty(),
                    extra =
                        about?.genres?.takeIf { it.isNotEmpty() }?.let { genres ->
                            {
                                FlowRow(
                                    horizontalArrangement = Arrangement.spacedBy(6.dp, androidx.compose.ui.Alignment.CenterHorizontally),
                                    verticalArrangement = Arrangement.spacedBy(6.dp),
                                    modifier = Modifier.fillMaxWidth(),
                                ) {
                                    genres.take(GENRES_SHOWN).forEach { Tag(it) }
                                }
                            }
                        },
                    coverShape = CircleShape,
                ) { side ->
                    Portrait(picture, portraitSong, side)
                }
            }
            if (shown.albums.isNotEmpty()) {
                item("albums-title") { SectionHeader("Albums") }
                item("albums") {
                    Shelf(shown.albums, key = { it.key }) { album ->
                        AlbumCard(album, onClick = { navigator.open(album.screen) }, modifier = Modifier.width(Shelves.albumWidth))
                    }
                }
            }
            if (tracks.isEmpty()) item("empty") { EmptyCollection(detail = "No songs by this artist are in your library.") }
            else {
                val showing = if (everySong || tracks.size <= SONGS_FOLDED) tracks else tracks.take(SONGS_FOLDED)
                item("songs-title") {
                    SectionHeader(
                        "Songs",
                        action =
                            when {
                                tracks.size <= SONGS_FOLDED -> null
                                everySong -> "Show fewer"
                                else -> "Show all ${tracks.size}"
                            },
                        onAction = { everySong = !everySong },
                    )
                }
                showing.forEachIndexed { index, track ->
                    item("song:$index:${track.id}") {
                        TrackRow(
                            track,
                            onClick = { resonate.playback.play(tracks, index) },
                            playing = track.id == playing,
                            supporting = track.album.ifBlank { track.artist },
                            onMore = { sheets.song(track) },
                        )
                    }
                }
            }
            val links = about?.links.orEmpty().distinct()
            if (links.isNotEmpty()) {
                item("links-title") { SectionHeader("Elsewhere") }
                links.forEach { link ->
                    item("link:$link") {
                        ListItem(
                            modifier = Modifier.clickable { resonate.host.open(Uri.parse(link)) },
                            colors = ListItemDefaults.colors(containerColor = Color.Transparent),
                            leadingContent = { Icon(Icons.Rounded.Link, contentDescription = null) },
                            supportingContent = { Text(link, maxLines = 1, overflow = TextOverflow.Ellipsis) },
                        ) {
                            Text(siteOf(link), maxLines = 1, overflow = TextOverflow.Ellipsis)
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun Portrait(picture: android.graphics.Bitmap?, song: ULong?, side: Dp) {
    if (picture != null) Picture(picture, Modifier.size(side), CircleShape, Icons.Rounded.Person)
    else ArtistPortrait(song, side)
}

@Composable
private fun Tag(text: String) {
    Surface(
        color = MaterialTheme.colorScheme.secondaryContainer,
        contentColor = MaterialTheme.colorScheme.onSecondaryContainer,
        shape = MaterialTheme.shapes.small,
    ) {
        Text(text, style = MaterialTheme.typography.labelLarge, modifier = Modifier.padding(horizontal = 10.dp, vertical = 4.dp))
    }
}

fun siteOf(link: String): String =
    link.substringAfter("://").substringBefore('/').removePrefix("www.").ifBlank { link }

private const val SONGS_FOLDED = 5
private const val GENRES_SHOWN = 6
