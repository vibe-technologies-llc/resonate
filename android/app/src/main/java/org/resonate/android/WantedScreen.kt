package org.resonate.android

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import org.resonate.android.nativecore.SearchSong

@Composable
fun WantedScreen(library: LibraryRepository) {
    val wanted by library.features.wanted.collectAsState()
    val online by library.features.online.collectAsState()
    val downloading by library.features.downloading.collectAsState()
    val status by library.features.downloadStatus.collectAsState()
    var missingOnly by remember { mutableStateOf(true) }
    var download by remember { mutableStateOf<SearchSong?>(null) }
    LaunchedEffect(Unit) {
        library.features.loadWanted()
        library.features.refreshFolders()
    }
    LazyColumn(
        Modifier.fillMaxSize().padding(horizontal = 20.dp, vertical = 8.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        item {
            Text(
                "Saved for later",
                style = MaterialTheme.typography.headlineSmall,
                fontWeight = FontWeight.Bold,
            )
            Text(
                "Keep recordings you want and download them when your provider has a match.",
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                style = MaterialTheme.typography.bodyMedium,
            )
        }
        item {
            Surface(
                shape = RoundedCornerShape(20.dp),
                color = MaterialTheme.colorScheme.surfaceContainerLow,
                border = BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.25f)),
                modifier = Modifier.fillMaxWidth(),
            ) {
                Row(
                    Modifier.padding(horizontal = 16.dp, vertical = 12.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(
                        "Missing from library only",
                        Modifier.weight(1f),
                        style = MaterialTheme.typography.titleMedium,
                    )
                    Switch(missingOnly, { missingOnly = it })
                }
            }
        }
        item {
            TextButton(onClick = library.features::loadWanted) { Text("Refresh library matches") }
        }
        if (!online) item { Text("Enable online features in Settings to download music.") }
        status?.let {
            item {
                Surface(
                    shape = RoundedCornerShape(16.dp),
                    color = MaterialTheme.colorScheme.primaryContainer.copy(alpha = 0.35f),
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Column(Modifier.padding(14.dp)) {
                        Text(it, style = MaterialTheme.typography.bodyMedium)
                        if (downloading)
                            TextButton(onClick = library.features::cancelDownload) {
                                Text("Cancel download")
                            }
                    }
                }
            }
        }
        val visible = wanted.filter { !missingOnly || !it.held }
        if (visible.isEmpty())
            item {
                Text(
                    if (wanted.isEmpty()) "Save a recording from Online search to find it here."
                    else "All saved recordings are in your library.",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        items(visible, key = { it.id }) { wantedSong ->
            Surface(
                shape = RoundedCornerShape(18.dp),
                color = MaterialTheme.colorScheme.surfaceContainerLow,
                border = BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.2f)),
                modifier = Modifier.fillMaxWidth(),
            ) {
                Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                    Text(
                        wantedSong.song.title,
                        style = MaterialTheme.typography.titleMedium,
                        fontWeight = FontWeight.SemiBold,
                    )
                    Text(
                        wantedSong.song.artist,
                        style = MaterialTheme.typography.bodyMedium,
                    )
                    if (wantedSong.song.album.isNotBlank())
                        Text(
                            wantedSong.song.album,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            style = MaterialTheme.typography.bodySmall,
                        )
                    if (wantedSong.held)
                        Surface(
                            shape = RoundedCornerShape(8.dp),
                            color = MaterialTheme.colorScheme.primaryContainer.copy(alpha = 0.5f),
                        ) {
                            Text(
                                "Already in library",
                                modifier = Modifier.padding(horizontal = 8.dp, vertical = 2.dp),
                                style = MaterialTheme.typography.labelSmall,
                                fontWeight = FontWeight.Bold,
                                color = MaterialTheme.colorScheme.primary,
                            )
                        }
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        TextButton(
                            onClick = { download = wantedSong.song },
                            enabled = online && !downloading && !wantedSong.held,
                        ) {
                            Text("Download")
                        }
                        TextButton(onClick = { library.features.removeWanted(wantedSong.id) }) {
                            Text("Remove")
                        }
                    }
                }
            }
        }
    }
    download?.let { song -> DownloadFolderDialog(library, song) { download = null } }
}
