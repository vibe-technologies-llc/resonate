package org.resonate.android

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Clear
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.Search
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Checkbox
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.OutlinedTextFieldDefaults
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import org.resonate.android.nativecore.MusicPlaylist
import org.resonate.android.nativecore.SyncStanding
import org.resonate.android.nativecore.Track

@Composable
fun PlaylistScreen(
    library: LibraryRepository,
    id: ULong,
    play: (List<Track>, Int, Boolean) -> Unit,
    back: () -> Unit,
    inspect: (Track) -> Unit,
) {
    val playlists by library.features.playlists.collectAsState()
    val selected by library.features.openPlaylist.collectAsState()
    val tracks by library.features.playlistRows.collectAsState()
    val snapshot by library.snapshot.collectAsState()
    val playing =
        snapshot?.transport == org.resonate.android.nativecore.Transport.PLAYING
    val syncStatus by library.features.syncStatus.collectAsState()
    val playlistSync by library.features.playlistSync.collectAsState()
    val playlist = playlists.find { it.id == id }
    val rows = if (selected == id) tracks else emptyList()
    var menu by remember { mutableStateOf(false) }
    var naming by remember { mutableStateOf(false) }
    var deleting by remember { mutableStateOf(false) }
    var adding by remember { mutableStateOf(false) }
    var picking by remember { mutableStateOf<Track?>(null) }
    var available by remember(id) { mutableStateOf(false) }
    LaunchedEffect(id) {
        library.features.loadPlaylists()
        library.features.selectPlaylist(id)
    }
    LaunchedEffect(playlist, selected) {
        if (playlist != null) available = true else if (available && selected == null) back()
    }
    LaunchedEffect(menu) { if (menu) library.features.loadPlaylistSync(id) }
    if (playlist == null) return
    val syncing = syncStatus?.standing.let { it != null && it != SyncStanding.OFF }
    val synced = playlistSync?.takeIf { it.playlist == id }
    val connected = syncStatus?.standing == SyncStanding.CONNECTED
    val lenders =
        syncStatus?.devices.orEmpty().filter {
            !it.thisDevice && (it.nearby || connected && it.online)
        }
    LazyColumn(
        Modifier.fillMaxSize(),
        contentPadding = PaddingValues(horizontal = 12.dp, vertical = 8.dp),
        verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        item(key = "header") {
            CollectionHeader(
                playlist.name,
                "Playlist · ${collectionSummary(rows)}",
                { PlaylistArtwork(playlist, library, Modifier.size(176.dp)) },
                { play(rows, 0, false) },
                { play(rows, rows.indices.random(), true) },
                rows.isNotEmpty(),
                track = rows.firstOrNull(),
                library = library,
            )
            Row(
                Modifier.fillMaxWidth().padding(horizontal = 8.dp, vertical = 8.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                FilledTonalButton(
                    onClick = { adding = true },
                    shape = RoundedCornerShape(16.dp),
                ) {
                    Icon(Icons.Default.Add, null)
                    Text("Add songs", Modifier.padding(start = 8.dp), fontWeight = FontWeight.SemiBold)
                }
                Spacer(Modifier.weight(1f))
                Column {
                    IconButton(onClick = { menu = true }) {
                        Icon(Icons.Default.MoreVert, "Playlist options")
                    }
                    DropdownMenu(menu, { menu = false }, shape = RoundedCornerShape(18.dp)) {
                        DropdownMenuItem(
                            text = { Text("Rename playlist") },
                            onClick = {
                                menu = false
                                naming = true
                            },
                        )
                        DropdownMenuItem(
                            text = { Text("Undo last playlist edit") },
                            onClick = {
                                menu = false
                                library.features.editPlaylist { it.undoPlaylist() }
                            },
                        )
                        DropdownMenuItem(
                            text = { Text("Delete playlist") },
                            onClick = {
                                menu = false
                                deleting = true
                            },
                        )
                        if (synced != null && synced.missing > 0u)
                            lenders.forEach { device ->
                                DropdownMenuItem(
                                    text = {
                                        Text(
                                            "Fetch ${synced.missing} missing ${if (synced.missing == 1u) "song" else "songs"} from ${device.name}"
                                        )
                                    },
                                    onClick = {
                                        menu = false
                                        library.features.fetchMissing(id, device.name)
                                    },
                                )
                            }
                        if (syncing)
                            DropdownMenuItem(
                                text = { Text("Keep its songs here") },
                                trailingIcon = {
                                    Checkbox(checked = synced?.kept == true, onCheckedChange = null)
                                },
                                onClick = {
                                    menu = false
                                    library.features.keepSongs(id, synced?.kept != true)
                                },
                            )
                    }
                }
            }
        }
        if (rows.isEmpty())
            item(key = "empty") {
                CollectionMessage(
                    "Your playlist starts here. Add songs from your library to make it yours."
                )
            }
        itemsIndexed(rows, key = { index, track -> "$index:${track.id}" }) { index, track ->
            var options by remember { mutableStateOf(false) }
            CollectionTrackRow(
                track,
                index,
                library,
                snapshot?.current?.id == track.id && playing,
                { play(rows, index, false) },
                modifier = Modifier.animateItem(),
            ) {
                Column {
                    IconButton(onClick = { options = true }) {
                        Icon(
                            Icons.Default.MoreVert,
                            "Options for song ${index + 1}: ${track.title}",
                        )
                    }
                    DropdownMenu(options, { options = false }, shape = RoundedCornerShape(18.dp)) {
                        DropdownMenuItem(
                            text = { Text("Add to another playlist") },
                            onClick = {
                                options = false
                                picking = track
                            },
                        )
                        DropdownMenuItem(
                            text = {
                                Text(
                                    if (track.favourite) "Remove from favourites"
                                    else "Add to favourites"
                                )
                            },
                            onClick = {
                                options = false
                                library.setFavourite(track.id, !track.favourite)
                            },
                        )
                        DropdownMenuItem(
                            text = { Text("Move up") },
                            enabled = index > 0,
                            onClick = {
                                options = false
                                library.features.editPlaylist {
                                    it.moveInPlaylist(
                                        playlist.id,
                                        playlist.revision,
                                        index.toUInt(),
                                        (index - 1).toUInt(),
                                    )
                                }
                            },
                        )
                        DropdownMenuItem(
                            text = { Text("Move down") },
                            enabled = index < rows.lastIndex,
                            onClick = {
                                options = false
                                library.features.editPlaylist {
                                    it.moveInPlaylist(
                                        playlist.id,
                                        playlist.revision,
                                        index.toUInt(),
                                        (index + 1).toUInt(),
                                    )
                                }
                            },
                        )
                        DropdownMenuItem(
                            text = { Text("Song details") },
                            onClick = {
                                options = false
                                inspect(track)
                            },
                        )
                        DropdownMenuItem(
                            text = { Text("Remove from playlist") },
                            onClick = {
                                options = false
                                library.features.editPlaylist {
                                    it.removeFromPlaylist(
                                        playlist.id,
                                        playlist.revision,
                                        index.toUInt(),
                                    )
                                }
                            },
                        )
                    }
                }
            }
        }
    }
    if (naming) PlaylistNameDialog(library, playlist, dismiss = { naming = false })
    if (adding) AddPlaylistSongs(library, playlist, dismiss = { adding = false })
    picking?.let { track -> PlaylistPicker(library, track, dismiss = { picking = null }) }
    if (deleting)
        AlertDialog(
            onDismissRequest = { deleting = false },
            shape = RoundedCornerShape(28.dp),
            title = { Text("Delete ${playlist.name}?") },
            text = {
                Text(
                    "The music files stay in your library. You can undo this action from a playlist's options."
                )
            },
            confirmButton = {
                TextButton(
                    onClick = {
                        library.features.editPlaylist {
                            it.deletePlaylist(playlist.id, playlist.revision)
                        }
                        deleting = false
                    }
                ) {
                    Text("Delete playlist")
                }
            },
            dismissButton = { TextButton(onClick = { deleting = false }) { Text("Cancel") } },
        )
}

@Composable
fun PlaylistNameDialog(
    library: LibraryRepository,
    playlist: MusicPlaylist? = null,
    dismiss: () -> Unit,
) {
    var name by rememberSaveable { mutableStateOf(playlist?.name.orEmpty()) }
    AlertDialog(
        onDismissRequest = dismiss,
        shape = RoundedCornerShape(28.dp),
        title = { Text(if (playlist == null) "Create playlist" else "Rename playlist") },
        text = {
            OutlinedTextField(
                name,
                { name = it },
                label = { Text("Playlist name") },
                singleLine = true,
                shape = RoundedCornerShape(16.dp),
            )
        },
        confirmButton = {
            TextButton(
                onClick = {
                    if (playlist == null) library.features.createPlaylist(name.trim())
                    else
                        library.features.editPlaylist {
                            it.renamePlaylist(playlist.id, playlist.revision, name.trim())
                        }
                    dismiss()
                },
                enabled = name.isNotBlank(),
            ) {
                Text(if (playlist == null) "Create" else "Save")
            }
        },
        dismissButton = { TextButton(onClick = dismiss) { Text("Cancel") } },
    )
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun AddPlaylistSongs(
    library: LibraryRepository,
    playlist: MusicPlaylist,
    dismiss: () -> Unit,
) {
    val tracks by library.allTracks.collectAsState()
    var query by rememberSaveable { mutableStateOf("") }
    var selected by rememberSaveable { mutableStateOf<List<String>>(emptyList()) }
    val visible = tracks.filter { "${it.title} ${it.artist} ${it.album}".contains(query, true) }
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    ModalBottomSheet(
        onDismissRequest = dismiss,
        sheetState = sheetState,
        shape = RoundedCornerShape(topStart = 28.dp, topEnd = 28.dp),
    ) {
        Column(
            Modifier.fillMaxHeight(0.88f).imePadding(),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Text(
                "Add to ${playlist.name}",
                Modifier.padding(horizontal = 20.dp),
                style = MaterialTheme.typography.titleLarge,
                fontWeight = FontWeight.Bold,
            )
            OutlinedTextField(
                query,
                { query = it },
                placeholder = { Text("Find songs to add") },
                leadingIcon = { Icon(Icons.Default.Search, null) },
                trailingIcon = {
                    if (query.isNotEmpty())
                        IconButton(onClick = { query = "" }) {
                            Icon(Icons.Default.Clear, "Clear search")
                        }
                },
                singleLine = true,
                shape = RoundedCornerShape(28.dp),
                colors =
                    OutlinedTextFieldDefaults.colors(
                        focusedContainerColor = MaterialTheme.colorScheme.surfaceContainerHigh,
                        unfocusedContainerColor = MaterialTheme.colorScheme.surfaceContainerHigh,
                        focusedBorderColor = MaterialTheme.colorScheme.primary,
                        unfocusedBorderColor = Color.Transparent,
                    ),
                modifier =
                    Modifier.fillMaxWidth().padding(horizontal = 20.dp).semantics {
                        contentDescription = "Find songs to add"
                    },
            )
            LazyColumn(
                Modifier.weight(1f),
                contentPadding = PaddingValues(horizontal = 12.dp, vertical = 4.dp),
                verticalArrangement = Arrangement.spacedBy(4.dp),
            ) {
                if (visible.isEmpty())
                    item {
                        CollectionMessage(
                            if (tracks.isEmpty())
                                "Add a music folder in Settings to start your library."
                            else "No songs match “$query”"
                        )
                    }
                items(visible, key = { it.id.toString() }) { track ->
                    val id = track.id.toString()
                    val checked = id in selected
                    Surface(
                        shape = RoundedCornerShape(16.dp),
                        color =
                            if (checked) MaterialTheme.colorScheme.primaryContainer.copy(alpha = 0.25f)
                            else Color.Transparent,
                        border =
                            if (checked)
                                BorderStroke(
                                    1.dp,
                                    MaterialTheme.colorScheme.primary.copy(alpha = 0.35f),
                                )
                            else null,
                        modifier = Modifier.fillMaxWidth().animateItem(),
                    ) {
                        Row(
                            Modifier.fillMaxWidth()
                                .toggleable(
                                    checked,
                                    role = Role.Checkbox,
                                    onValueChange = {
                                        selected = if (it) selected + id else selected - id
                                    },
                                )
                                .padding(horizontal = 12.dp, vertical = 8.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Artwork(Modifier.size(44.dp), track, library, RoundedCornerShape(10.dp))
                            Column(Modifier.weight(1f).padding(horizontal = 12.dp)) {
                                Text(
                                    track.title,
                                    maxLines = 1,
                                    overflow = TextOverflow.Ellipsis,
                                    style = MaterialTheme.typography.titleMedium,
                                    fontWeight = if (checked) FontWeight.Bold else FontWeight.Medium,
                                )
                                Text(
                                    track.artist,
                                    maxLines = 1,
                                    overflow = TextOverflow.Ellipsis,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    style = MaterialTheme.typography.bodyMedium,
                                )
                            }
                            Checkbox(checked, onCheckedChange = null)
                        }
                    }
                }
            }
            Button(
                onClick = {
                    library.features.addToPlaylist(
                        playlist.id,
                        tracks.filter { it.id.toString() in selected },
                    )
                    dismiss()
                },
                enabled = selected.isNotEmpty(),
                shape = RoundedCornerShape(18.dp),
                modifier = Modifier.fillMaxWidth().height(52.dp).padding(horizontal = 20.dp, vertical = 2.dp),
            ) {
                Text(
                    if (selected.size == 1) "Add 1 song" else "Add ${selected.size} songs",
                    fontWeight = FontWeight.SemiBold,
                )
            }
            Spacer(Modifier.height(8.dp))
        }
    }
}

@Composable
fun PlaylistPicker(library: LibraryRepository, track: Track, dismiss: () -> Unit) {
    val playlists by library.features.playlists.collectAsState()
    var name by rememberSaveable { mutableStateOf("") }
    LaunchedEffect(Unit) { library.features.loadPlaylists() }
    AlertDialog(
        onDismissRequest = dismiss,
        shape = RoundedCornerShape(28.dp),
        title = { Text("Add to playlist") },
        text = {
            LazyColumn(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                items(playlists, key = { it.id.toString() }) { playlist ->
                    CollectionRow(
                        playlist.name,
                        songCount(playlist.songs.toInt()),
                        artwork = { PlaylistArtwork(playlist, library, Modifier.size(44.dp)) },
                    ) {
                        library.features.addToPlaylist(playlist.id, track)
                        dismiss()
                    }
                }
                item {
                    OutlinedTextField(
                        name,
                        { name = it },
                        label = { Text("New playlist name") },
                        singleLine = true,
                        shape = RoundedCornerShape(16.dp),
                        modifier = Modifier.fillMaxWidth().padding(top = 8.dp),
                    )
                }
            }
        },
        confirmButton = {
            TextButton(
                onClick = {
                    library.features.createPlaylist(name.trim(), track)
                    dismiss()
                },
                enabled = name.isNotBlank(),
            ) {
                Text("Create and add")
            }
        },
        dismissButton = { TextButton(onClick = dismiss) { Text("Cancel") } },
    )
}
