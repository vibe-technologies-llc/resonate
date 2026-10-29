package org.resonate.android.ui.server

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.PlaylistPlay
import androidx.compose.material.icons.automirrored.rounded.QueueMusic
import androidx.compose.material.icons.rounded.MusicNote
import androidx.compose.material.icons.rounded.Person
import androidx.compose.material.icons.rounded.PlayArrow
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import org.resonate.android.nativecore.ServedAlbum
import org.resonate.android.nativecore.ServedArtist
import org.resonate.android.nativecore.ServedSong
import org.resonate.android.ui.components.CollectionRow
import org.resonate.android.ui.components.Picture
import org.resonate.android.ui.components.Sizes
import org.resonate.android.ui.components.rememberServedCover
import org.resonate.android.ui.library.CoverCard
import org.resonate.android.ui.library.MenuEntry
import org.resonate.android.ui.library.OverflowMenu
import org.resonate.android.ui.library.Shelves
import org.resonate.android.ui.navigation.Screen

@Composable
fun ServedCover(
    uri: String,
    cover: String?,
    size: Dp,
    modifier: Modifier = Modifier,
    shape: Shape = MaterialTheme.shapes.medium,
    placeholder: ImageVector = Icons.Rounded.MusicNote,
) {
    val edge = with(LocalDensity.current) { size.roundToPx() }
    Picture(
        bitmap = rememberServedCover(uri, cover, edge),
        modifier = modifier.size(size),
        shape = shape,
        placeholder = placeholder,
    )
}

@Composable
fun ServedSquare(uri: String, cover: String?, modifier: Modifier = Modifier) {
    BoxWithConstraints(modifier.fillMaxWidth().aspectRatio(1f)) {
        ServedCover(uri, cover, maxWidth, shape = MaterialTheme.shapes.large)
    }
}

@Composable
fun ServedMenu(uri: String, actions: ServerActions, description: String) {
    OverflowMenu(
        listOf(
            listOf(
                MenuEntry.Action("Play", Icons.Rounded.PlayArrow) { actions.play(uri) },
                MenuEntry.Action("Play next", Icons.AutoMirrored.Rounded.PlaylistPlay) {
                    actions.playNext(uri)
                },
                MenuEntry.Action("Add to queue", Icons.AutoMirrored.Rounded.QueueMusic) {
                    actions.addToQueue(uri)
                },
            )
        ),
        description = description,
    )
}

fun ServedArtist.screen(): Screen.ServedArtist = Screen.ServedArtist(uri, shownName, cover)

fun ServedAlbum.screen(): Screen.ServedAlbum = Screen.ServedAlbum(uri, shownName, artist.orEmpty(), cover)

@Composable
fun ServedArtistRow(artist: ServedArtist, onClick: () -> Unit, actions: ServerActions) {
    CollectionRow(
        title = artist.shownName,
        supporting = artistLine(artist),
        onClick = onClick,
        leading = {
            ServedCover(
                artist.uri,
                artist.cover,
                Sizes.rowArt,
                shape = CircleShape,
                placeholder = Icons.Rounded.Person,
            )
        },
        trailing = { ServedMenu(artist.uri, actions, "Options for ${artist.shownName}") },
        modifier = Modifier.testTag("served-artist"),
    )
}

@Composable
fun ServedAlbumRow(album: ServedAlbum, onClick: () -> Unit, actions: ServerActions) {
    CollectionRow(
        title = album.shownName,
        supporting = albumLine(album).ifBlank { null },
        onClick = onClick,
        leading = { ServedCover(album.uri, album.cover, Sizes.rowArt) },
        trailing = { ServedMenu(album.uri, actions, "Options for ${album.shownName}") },
        modifier = Modifier.testTag("served-album"),
    )
}

@Composable
fun ServedAlbumCard(album: ServedAlbum, onClick: () -> Unit, modifier: Modifier = Modifier) {
    CoverCard(
        title = album.shownName,
        detail = albumLine(album).ifBlank { null },
        onClick = onClick,
        modifier = modifier.width(Shelves.albumWidth).testTag("served-album"),
    ) {
        ServedSquare(album.uri, album.cover)
    }
}

@Composable
fun ServedSongRow(
    song: ServedSong,
    onClick: () -> Unit,
    actions: ServerActions,
    number: String? = null,
) {
    CollectionRow(
        title = song.shownTitle,
        supporting = songLine(song, withAlbum = number == null).ifBlank { null },
        onClick = onClick,
        leading = {
            if (number != null) {
                Box(Modifier.width(32.dp), contentAlignment = Alignment.Center) {
                    Text(
                        number,
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        textAlign = TextAlign.Center,
                    )
                }
            } else {
                ServedCover(song.uri, song.cover, Sizes.rowArt)
            }
        },
        trailing = { ServedMenu(song.uri, actions, "Options for ${song.shownTitle}") },
        modifier = Modifier.testTag("served-song"),
    )
}
