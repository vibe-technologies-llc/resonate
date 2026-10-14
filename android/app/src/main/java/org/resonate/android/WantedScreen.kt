package org.resonate.android

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.MaterialTheme
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
        Modifier.fillMaxSize().padding(20.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        item {
            Text("Saved for later", style = MaterialTheme.typography.headlineSmall)
            Text(
                "Keep recordings you want and download them when your provider has a match.",
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        item {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text("Missing from library only", Modifier.weight(1f))
                Switch(missingOnly, { missingOnly = it })
            }
        }
        item {
            TextButton(onClick = library.features::loadWanted) { Text("Refresh library matches") }
        }
        if (!online) item { Text("Enable online features in Settings to download music.") }
        status?.let {
            item {
                Text(it, style = MaterialTheme.typography.bodySmall)
                if (downloading)
                    TextButton(onClick = library.features::cancelDownload) {
                        Text("Cancel download")
                    }
            }
        }
        val visible = wanted.filter { !missingOnly || !it.held }
        if (visible.isEmpty())
            item {
                Text(
                    if (wanted.isEmpty()) "Save a recording from Online search to find it here."
                    else "All saved recordings are in your library."
                )
            }
        items(visible, key = { it.id }) { wantedSong ->
            Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Text(wantedSong.song.title, style = MaterialTheme.typography.titleMedium)
                Text(wantedSong.song.artist)
                if (wantedSong.song.album.isNotBlank())
                    Text(wantedSong.song.album, color = MaterialTheme.colorScheme.onSurfaceVariant)
                if (wantedSong.held)
                    Text("Already in library", color = MaterialTheme.colorScheme.primary)
                Row {
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
    download?.let { song -> DownloadFolderDialog(library, song) { download = null } }
}
