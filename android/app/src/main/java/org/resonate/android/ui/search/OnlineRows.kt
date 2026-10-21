package org.resonate.android.ui.search

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.BookmarkAdd
import androidx.compose.material.icons.rounded.Album
import androidx.compose.material.icons.rounded.CheckCircle
import androidx.compose.material.icons.rounded.DeleteOutline
import androidx.compose.material.icons.rounded.Download
import androidx.compose.material.icons.rounded.LibraryMusic
import androidx.compose.material.icons.rounded.MoreVert
import androidx.compose.material.icons.rounded.MusicNote
import androidx.compose.material.icons.rounded.Person
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import org.resonate.android.nativecore.SearchSong
import org.resonate.android.ui.clock
import org.resonate.android.ui.components.IconTile
import org.resonate.android.ui.components.SheetRow
import org.resonate.android.ui.components.Sizes

class SongAction(val icon: ImageVector, val label: String, val run: () -> Unit)

fun songLine(song: SearchSong, withAlbum: Boolean = true): String =
    listOfNotNull(
            song.artist.takeIf { it.isNotBlank() },
            song.album.takeIf { withAlbum && it.isNotBlank() },
            song.durationMs?.let { clock(it) },
        )
        .joinToString(" · ")

fun onlineSongActions(
    save: () -> Unit,
    download: () -> Unit,
    album: (() -> Unit)?,
    artist: (() -> Unit)?,
): List<SongAction> = buildList {
    add(SongAction(Icons.Outlined.BookmarkAdd, "Save for later", save))
    add(SongAction(Icons.Rounded.Download, "Download…", download))
    album?.let { add(SongAction(Icons.Rounded.Album, "Go to album", it)) }
    artist?.let { add(SongAction(Icons.Rounded.Person, "Go to artist", it)) }
}

fun wantedSongActions(download: () -> Unit, remove: () -> Unit): List<SongAction> =
    listOf(
        SongAction(Icons.Rounded.Download, "Download…", download),
        SongAction(Icons.Rounded.DeleteOutline, "Remove from saved", remove),
    )

@Composable
fun SongTile(held: Boolean, saved: Boolean = false) {
    if (held) {
        IconTile(
            Icons.Rounded.LibraryMusic,
            container = MaterialTheme.colorScheme.primaryContainer,
            content = MaterialTheme.colorScheme.onPrimaryContainer,
        )
    } else {
        IconTile(if (saved) Icons.Outlined.BookmarkAdd else Icons.Rounded.MusicNote)
    }
}

@Composable
fun HeldLabel() {
    Row(verticalAlignment = Alignment.CenterVertically) {
        Icon(
            Icons.Rounded.CheckCircle,
            contentDescription = null,
            tint = MaterialTheme.colorScheme.primary,
            modifier = Modifier.size(14.dp),
        )
        Spacer(Modifier.width(4.dp))
        Text(
            "In your library",
            style = MaterialTheme.typography.labelMedium,
            color = MaterialTheme.colorScheme.primary,
            maxLines = 1,
        )
    }
}

@Composable
fun OnlineSongRow(
    song: SearchSong,
    held: Boolean,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    number: String? = null,
    saved: Boolean = false,
) {
    ListItem(
        modifier = modifier.clickable(onClick = onClick).testTag("online-song"),
        colors = ListItemDefaults.colors(containerColor = Color.Transparent),
        leadingContent = {
            if (number != null) {
                Box(Modifier.width(32.dp), contentAlignment = Alignment.Center) {
                    Text(
                        number,
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            } else {
                SongTile(held, saved)
            }
        },
        supportingContent = {
            Column {
                Text(songLine(song, withAlbum = number == null), maxLines = 1, overflow = TextOverflow.Ellipsis)
                if (held) HeldLabel()
            }
        },
        trailingContent = {
            IconButton(onClick = onClick) {
                Icon(Icons.Rounded.MoreVert, contentDescription = "More options for ${song.title}")
            }
        },
    ) {
            Text(song.title.ifBlank { "Unknown song" }, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
}

@Composable
fun OnlineCollectionRow(
    title: String,
    detail: String,
    icon: ImageVector,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    round: Boolean = false,
) {
    ListItem(
        modifier = modifier.clickable(onClick = onClick),
        colors = ListItemDefaults.colors(containerColor = Color.Transparent),
        leadingContent = {
            if (round) {
                Surface(
                    color = MaterialTheme.colorScheme.tertiaryContainer,
                    contentColor = MaterialTheme.colorScheme.onTertiaryContainer,
                    shape = CircleShape,
                    modifier = Modifier.size(Sizes.rowArt),
                ) {
                    Box(contentAlignment = Alignment.Center) { Icon(icon, contentDescription = null) }
                }
            } else {
                IconTile(icon)
            }
        },
        supportingContent =
            detail.takeIf { it.isNotBlank() }?.let { { Text(it, maxLines = 1, overflow = TextOverflow.Ellipsis) } },
    ) { Text(title.ifBlank { "Untitled" }, maxLines = 2, overflow = TextOverflow.Ellipsis) }
}

@Composable
fun OnlineSongSheet(song: SearchSong, held: Boolean, dismiss: () -> Unit, actions: List<SongAction>) {
    ModalBottomSheet(onDismissRequest = dismiss) {
        Column(
            Modifier.verticalScroll(rememberScrollState())
                .navigationBarsPadding()
                .testTag("online-song-sheet")
        ) {
            ListItem(
                colors = ListItemDefaults.colors(containerColor = Color.Transparent),
                leadingContent = { SongTile(held) },
                supportingContent = {
                    Column {
                        Text(songLine(song), maxLines = 2, overflow = TextOverflow.Ellipsis)
                        if (held) HeldLabel()
                    }
                },
            ) {
                    Text(
                        song.title.ifBlank { "Unknown song" },
                        style = MaterialTheme.typography.titleMedium,
                        maxLines = 2,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
            HorizontalDivider(Modifier.padding(vertical = 8.dp))
            actions.forEach { action ->
                SheetRow(action.icon, action.label) {
                    dismiss()
                    action.run()
                }
            }
            Spacer(Modifier.padding(bottom = 8.dp))
        }
    }
}
