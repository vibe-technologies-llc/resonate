package org.resonate.android.ui.server

import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.PlaylistPlay
import androidx.compose.material.icons.automirrored.rounded.QueueMusic
import androidx.compose.material.icons.rounded.Person
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.resonate.android.ui.components.SectionHeader
import org.resonate.android.ui.components.rememberServedCover
import org.resonate.android.ui.library.ArtistLink
import org.resonate.android.ui.library.CollectionHeader
import org.resonate.android.ui.library.CollectionPage
import org.resonate.android.ui.library.EmptyCollection
import org.resonate.android.ui.library.MenuEntry
import org.resonate.android.ui.library.OverflowMenu
import org.resonate.android.ui.library.Shelf
import org.resonate.android.ui.navigation.LocalNavigator
import org.resonate.android.ui.navigation.Screen
import org.resonate.android.ui.resonateViewModel
import org.resonate.android.ui.theme.ArtworkTheme
import org.resonate.android.ui.theme.rememberSeedOf

private const val SEED_EDGE = 96

@Composable
fun ServedArtistScreen(screen: Screen.ServedArtist) {
    val model = resonateViewModel(key = "served-artist:${screen.uri}") {
        ServedAlbumsViewModel(it, screen.uri)
    }
    val navigator = LocalNavigator.current
    val actions = rememberServerActions()
    val loaded by model.albums.collectAsStateWithLifecycle()
    val seed = rememberSeedOf(rememberServedCover(screen.uri, screen.cover, SEED_EDGE))
    ArtworkTheme(seed) {
        CollectionPage(
            screen.name.ifBlank { "Unknown artist" },
            loaded,
            model.retry::again,
            tag = "served-artist-page",
            actions = {
                OverflowMenu(
                    listOf(
                        listOf(
                            MenuEntry.Action("Play next", Icons.AutoMirrored.Rounded.PlaylistPlay) {
                                actions.playNext(screen.uri)
                            },
                            MenuEntry.Action("Add to queue", Icons.AutoMirrored.Rounded.QueueMusic) {
                                actions.addToQueue(screen.uri)
                            },
                        )
                    ),
                    description = "Artist options",
                )
            },
        ) { albums ->
            item("header") {
                CollectionHeader(
                    title = screen.name.ifBlank { "Unknown artist" },
                    overline = "Artist",
                    summary = artistSummary(albums),
                    onPlay = { actions.play(screen.uri) },
                    onShuffle = { actions.shuffle(screen.uri) },
                    playable = albums.isNotEmpty(),
                    coverShape = CircleShape,
                ) { side ->
                    ServedCover(
                        screen.uri,
                        screen.cover,
                        side,
                        shape = CircleShape,
                        placeholder = Icons.Rounded.Person,
                    )
                }
            }
            if (albums.isEmpty()) {
                item("empty") { EmptyCollection(detail = "Nothing is listed for this artist.") }
            } else {
                item("albums-title") { SectionHeader("Albums") }
                item("albums") {
                    Shelf(albums, key = { it.uri }) { album ->
                        ServedAlbumCard(album, onClick = { navigator.open(album.screen()) })
                    }
                }
            }
        }
    }
}

@Composable
fun ServedAlbumScreen(screen: Screen.ServedAlbum) {
    val model = resonateViewModel(key = "served-album:${screen.uri}") {
        ServedSongsViewModel(it, screen.uri)
    }
    val actions = rememberServerActions()
    val loaded by model.songs.collectAsStateWithLifecycle()
    val title = screen.name.ifBlank { "Unknown album" }
    val seed = rememberSeedOf(rememberServedCover(screen.uri, screen.cover, SEED_EDGE))
    ArtworkTheme(seed) {
        CollectionPage(
            title,
            loaded,
            model.retry::again,
            tag = "served-album-page",
            actions = {
                OverflowMenu(
                    listOf(
                        listOf(
                            MenuEntry.Action("Play next", Icons.AutoMirrored.Rounded.PlaylistPlay) {
                                actions.playNext(screen.uri)
                            },
                            MenuEntry.Action("Add to queue", Icons.AutoMirrored.Rounded.QueueMusic) {
                                actions.addToQueue(screen.uri)
                            },
                        )
                    ),
                    description = "Album options",
                )
            },
        ) { shown ->
            item("header") {
                CollectionHeader(
                    title = title,
                    overline = "Album",
                    summary = albumSummary(shown),
                    onPlay = { actions.play(screen.uri) },
                    onShuffle = { actions.shuffle(screen.uri) },
                    playable = shown.isNotEmpty(),
                    link = {
                        if (screen.artist.isNotBlank()) ArtistLink(screen.artist, null, null)
                    },
                ) { side ->
                    ServedCover(
                        screen.uri,
                        screen.cover,
                        side,
                        shape = MaterialTheme.shapes.extraLarge,
                    )
                }
            }
            if (shown.isEmpty()) item("empty") { EmptyCollection(detail = "Nothing is listed for this album.") }
            shown.forEachIndexed { index, song ->
                item("row:$index:${song.uri}") {
                    ServedSongRow(
                        song,
                        onClick = { actions.playFrom(shown, index) },
                        actions = actions,
                        number = songNumber(song, index),
                    )
                }
            }
        }
    }
}
