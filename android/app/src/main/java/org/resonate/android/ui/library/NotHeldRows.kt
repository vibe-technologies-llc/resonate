package org.resonate.android.ui.library

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.width
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Download
import androidx.compose.material.icons.rounded.MoreVert
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import org.resonate.android.DownloadState
import org.resonate.android.nativecore.SearchSong
import org.resonate.android.ui.clock
import org.resonate.android.ui.components.rememberTitleMaxLines

const val NOT_IN_YOUR_LIBRARY = "Not in your library"

fun standingWords(standing: DownloadState?): String? =
    when (standing) {
        DownloadState.WAITING -> "Waiting to download"
        DownloadState.FINDING -> "Finding the song…"
        DownloadState.DOWNLOADING -> "Downloading…"
        DownloadState.VALIDATING -> "Checking the download…"
        DownloadState.DONE,
        DownloadState.FAILED,
        DownloadState.CANCELLED,
        null -> null
    }

fun notHeldLine(song: SearchSong, standing: DownloadState?, withAlbum: Boolean): String =
    standingWords(standing)
        ?: listOfNotNull(
                song.album.takeIf { withAlbum && it.isNotBlank() },
                song.durationMs?.let { clock(it) },
            )
            .joinToString(" · ")

@Composable
fun NotHeldSongRow(
    song: SearchSong,
    standing: DownloadState?,
    onDownload: () -> Unit,
    onMore: (() -> Unit)?,
    modifier: Modifier = Modifier,
    number: String? = null,
    withAlbum: Boolean = false,
) {
    val scheme = MaterialTheme.colorScheme
    val lines = rememberTitleMaxLines()
    val line = notHeldLine(song, standing, withAlbum)
    ListItem(
        onClick = { if (standing == null) onDownload() },
        modifier =
            modifier
                .testTag("not-held-song")
                .semantics(mergeDescendants = true) { stateDescription = NOT_IN_YOUR_LIBRARY },
        colors =
            ListItemDefaults.colors(containerColor = Color.Transparent, contentColor = scheme.onSurfaceVariant),
        leadingContent =
            number?.let { place ->
                {
                    Box(Modifier.width(32.dp), contentAlignment = Alignment.Center) {
                        Text(
                            place,
                            style = MaterialTheme.typography.bodyMedium,
                            color = scheme.onSurfaceVariant,
                            textAlign = TextAlign.Center,
                        )
                    }
                }
            },
        supportingContent =
            if (line.isEmpty()) null
            else {
                { Text(line, maxLines = 1, overflow = TextOverflow.Ellipsis) }
            },
        trailingContent = {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.End) {
                if (standing == null) {
                    IconButton(onClick = onDownload) {
                        Icon(Icons.Rounded.Download, contentDescription = "Download ${song.title}")
                    }
                }
                if (onMore != null) {
                    IconButton(onClick = onMore) {
                        Icon(Icons.Rounded.MoreVert, contentDescription = "More options for ${song.title}")
                    }
                }
            }
        },
    ) {
        Text(song.title.ifBlank { "Unknown song" }, maxLines = lines, overflow = TextOverflow.Ellipsis)
    }
}
