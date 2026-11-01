package org.resonate.android.ui.library

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.material3.ButtonDefaults
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.PlaylistAdd
import androidx.compose.material.icons.automirrored.rounded.QueueMusic
import androidx.compose.material.icons.automirrored.rounded.Undo
import androidx.compose.material.icons.rounded.CloudDownload
import androidx.compose.material.icons.rounded.Delete
import androidx.compose.material.icons.rounded.DeleteOutline
import androidx.compose.material.icons.rounded.DownloadForOffline
import androidx.compose.material.icons.rounded.Edit
import androidx.compose.material.icons.rounded.LinkOff
import androidx.compose.material.icons.rounded.MoreVert
import androidx.compose.material.icons.rounded.Refresh
import androidx.compose.material.icons.rounded.Sync
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material.icons.automirrored.rounded.PlaylistPlay
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.SnackbarDuration
import androidx.compose.material3.SnackbarResult
import androidx.compose.material3.SwipeToDismissBox
import androidx.compose.material3.SwipeToDismissBoxValue
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberSwipeToDismissBoxState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.activity.compose.LocalActivity
import androidx.compose.ui.Alignment
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.lifecycleScope
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import org.resonate.android.LinkedPlaylistWords
import org.resonate.android.nativecore.PlaylistLinkInfo
import org.resonate.android.ui.LocalResonate
import org.resonate.android.ui.components.DragHandle
import org.resonate.android.ui.components.LocalSheets
import org.resonate.android.ui.components.PlaylistCover
import org.resonate.android.ui.components.RowSelection
import org.resonate.android.ui.components.SelectionRemoval
import org.resonate.android.ui.components.SongSelectionBar
import org.resonate.android.ui.components.rememberSelection
import org.resonate.android.ui.components.TrackRow
import org.resonate.android.ui.components.rememberArtwork
import org.resonate.android.ui.components.rememberReorder
import org.resonate.android.ui.theme.ArtworkTheme
import org.resonate.android.ui.theme.rememberSeedOf
import org.resonate.android.ui.components.reorderActions
import org.resonate.android.ui.components.reorderable
import org.resonate.android.ui.counted
import org.resonate.android.ui.navigation.LocalNavigator
import org.resonate.android.ui.navigation.Screen
import org.resonate.android.ui.resonateViewModel
import org.resonate.android.ui.shownTitle

@Composable
fun PlaylistScreen(screen: Screen.Playlist) {
    val model = resonateViewModel(key = "playlist:${screen.id}") { PlaylistViewModel(it, screen.id) }
    val resonate = LocalResonate.current
    val navigator = LocalNavigator.current
    val sheets = LocalSheets.current
    val scope = rememberCoroutineScope()
    val lasting = (LocalActivity.current as? LifecycleOwner)?.lifecycleScope ?: scope
    val playlist by model.playlist.collectAsStateWithLifecycle()
    val loaded by model.rows.collectAsStateWithLifecycle()
    val sync by model.sync.collectAsStateWithLifecycle()
    val playing by model.playing.collectAsStateWithLifecycle()
    val refreshing by model.refreshing.collectAsStateWithLifecycle()
    val online by model.online.collectAsStateWithLifecycle()
    var adding by rememberSaveable { mutableStateOf(false) }
    var deleting by rememberSaveable { mutableStateOf(false) }
    val name = playlist?.name ?: screen.name
    val search = playlist?.search
    val link = playlist?.link
    val entries = loaded.valueOrNull().orEmpty()
    val tracks = entries.map { it.track }
    val list = rememberLazyListState()
    val selection = rememberSelection<String>()
    LaunchedEffect(entries) { selection.retain(entries.map { it.key }) }
    val reorder =
        rememberReorder(
            items = entries,
            list = list,
            keyOf = { it.key },
            firstIndex = { 1 },
            onMove = { from, to -> model.move(from, to, playlist?.revision) },
        )
    LaunchedEffect(screen.id) { model.open() }
    val seed = rememberSeedOf(rememberArtwork(playlist?.covers?.firstOrNull(), 0u, 96))
    ArtworkTheme(seed) {
        CollectionPage(
            name,
            loaded,
            model::open,
            tag = "playlist",
            list = list,
            selectionBar =
                if (!selection.active) null
                else {
                    {
                        SongSelectionBar(
                            tracks = selection.picked(entries) { it.key }.map { it.track },
                            onSelectAll = { selection.selectAll(entries.map { it.key }) },
                            onClose = selection::clear,
                            removal =
                                if (search != null) null
                                else
                                    SelectionRemoval("Remove from playlist") {
                                        model.removeAll(
                                            selection.picked(entries) { it.key }.map { it.position },
                                            playlist?.revision,
                                        )
                                    },
                        )
                    }
                },
            actions = {
                OverflowMenu(
                    playlistMenu(
                        sync.takeIf { search == null },
                        fills = search != null,
                        onPlayNext = { resonate.playNext(tracks) },
                        onQueue = { resonate.addToQueue(tracks) },
                        onAdd = { adding = true },
                        onRename = { sheets.name("Rename playlist", name) { model.rename(it) } },
                        onEditSearch =
                            search?.let { held ->
                                {
                                    sheets.editSearch("Edit saved search", SavedSearchDraft.of(name, held)) { renamed, revised ->
                                        model.revise(renamed, revised)
                                    }
                                }
                            },
                        onUndo = model::undo,
                        onKeep = model::keep,
                        onFetch = model::fetch,
                        link = link,
                        onKeepInStep = model::keepInStep,
                        onForgetLink = model::forgetLink,
                        onDelete = { deleting = true },
                    ),
                    description = "Playlist options",
                )
            },
        ) { rows ->
            item("header") {
                CollectionHeader(
                    title = name,
                    overline = if (search == null) "Playlist" else "Saved search",
                    summary = collectionSummary(tracks = rows.map { it.track }),
                    onPlay = { resonate.playInOrder(rows.map { it.track }) },
                    onShuffle = { resonate.playShuffled(rows.map { it.track }) },
                    playable = rows.isNotEmpty(),
                    extra = {
                        if (search != null) {
                            Column {
                                Text(
                                    search.text.ifBlank { "Every song" },
                                    style = MaterialTheme.typography.bodyMedium,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    modifier = Modifier.testTag("saved-search-text"),
                                )
                                Text(
                                    search.reading(),
                                    style = MaterialTheme.typography.bodySmall,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    modifier = Modifier.testTag("saved-search-reading"),
                                )
                            }
                        } else {
                            Column {
                                if (link != null) {
                                    LinkedPlaylistLine(
                                        link = link,
                                        refreshing = refreshing,
                                        online = online,
                                        onRefresh = model::refreshLink,
                                    )
                                }
                                if (rows.isEmpty()) {
                                    FilledTonalButton(onClick = { adding = true }, modifier = Modifier.testTag("add-songs-button")) {
                                        AddSongsLabel()
                                    }
                                } else {
                                    TextButton(onClick = { adding = true }, modifier = Modifier.testTag("add-songs-button")) {
                                        AddSongsLabel()
                                    }
                                }
                            }
                        }
                    },
                ) { side ->
                    PlaylistCover(playlist?.covers.orEmpty(), side, shape = MaterialTheme.shapes.extraLarge)
                }
            }
            if (search != null) {
                if (rows.isEmpty()) item("empty") { EmptyCollection(detail = "No song in your library matches this search yet.") }
                items(rows.size, key = { rows[it].key }) { at ->
                    val entry = rows[at]
                    TrackRow(
                        entry.track,
                        onClick = { resonate.playback.play(tracks, entry.position) },
                        playing = entry.track.id == playing,
                        trailing = {
                            IconButton(onClick = { sheets.song(entry.track) }) { Icon(Icons.Rounded.MoreVert, "More options") }
                        },
                        selection = selection.row(entry.key),
                    )
                }
                return@CollectionPage
            }
            if (rows.isEmpty()) item("empty") { EmptyCollection(detail = "Add songs to shape this playlist.") }
            items(reorder.shown.size, key = { reorder.shown[it].key }) { at ->
                val entry = reorder.shown[at]
                key(rows) {
                    Box(reorderable(reorder, entry.key)) {
                        PlaylistRow(
                            entry = entry,
                            last = rows.size - 1,
                            playing = entry.track.id == playing,
                            onPlay = { resonate.playback.play(tracks, entry.position) },
                            onMore = { sheets.song(entry.track) },
                            onRemove = { model.remove(entry.position, playlist?.revision) },
                            onMove = { to -> model.move(entry.position, to, playlist?.revision) },
                            selection = selection.row(entry.key),
                            handle = { DragHandle(reorder, entry.key, entry.track.shownTitle) },
                        )
                    }
                }
            }
        }
    }
    if (adding) {
        AddSongsSheet(
            catalog = model.catalog,
            already = tracks.map { it.id }.toSet(),
            onAdd = model::add,
            onDismiss = { adding = false },
        )
    }
    if (deleting) {
        AlertDialog(
            onDismissRequest = { deleting = false },
            icon = { Icon(Icons.Rounded.DeleteOutline, contentDescription = null) },
            title = { Text("Delete “$name”?") },
            text = { Text("Your music files will stay in your library.") },
            confirmButton = {
                TextButton(
                    onClick = {
                        deleting = false
                        model.delete()
                        navigator.back()
                        lasting.launch {
                            val answer =
                                sheets.snackbar.showSnackbar(
                                    "Deleted “$name”",
                                    actionLabel = "Undo",
                                    duration = SnackbarDuration.Long,
                                )
                            if (answer == SnackbarResult.ActionPerformed) model.undoDeletion()
                        }
                    }
                ) {
                    Text("Delete", color = MaterialTheme.colorScheme.error)
                }
            },
            dismissButton = { TextButton(onClick = { deleting = false }) { Text("Cancel") } },
        )
    }
}

@Composable
private fun LinkedPlaylistLine(
    link: PlaylistLinkInfo,
    refreshing: Boolean,
    online: Boolean,
    onRefresh: () -> Unit,
) {
    Row(verticalAlignment = Alignment.CenterVertically) {
        Text(
            LinkedPlaylistWords.line(link),
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.weight(1f, fill = false).testTag("playlist-link-line"),
        )
        Spacer(Modifier.width(8.dp))
        Box(Modifier.size(48.dp), contentAlignment = Alignment.Center) {
            if (refreshing) {
                CircularProgressIndicator(
                    Modifier.size(24.dp).semantics { contentDescription = "Refreshing from ${link.service}" }
                )
            } else {
                IconButton(
                    onClick = onRefresh,
                    enabled = online,
                    modifier = Modifier.testTag("playlist-link-refresh"),
                ) {
                    Icon(Icons.Rounded.Refresh, contentDescription = "Refresh from ${link.service}")
                }
            }
        }
    }
}

@Composable
private fun AddSongsLabel() {
    Icon(Icons.AutoMirrored.Rounded.PlaylistAdd, null, Modifier.size(ButtonDefaults.IconSize))
    Spacer(Modifier.width(ButtonDefaults.IconSpacing))
    Text("Add songs")
}

private fun playlistMenu(
    sync: PlaylistSyncView?,
    fills: Boolean,
    onPlayNext: () -> Unit,
    onQueue: () -> Unit,
    onAdd: () -> Unit,
    onRename: () -> Unit,
    onEditSearch: (() -> Unit)?,
    onUndo: () -> Unit,
    onKeep: (Boolean) -> Unit,
    onFetch: (org.resonate.android.nativecore.SyncedDevice) -> Unit,
    link: PlaylistLinkInfo?,
    onKeepInStep: (Boolean) -> Unit,
    onForgetLink: () -> Unit,
    onDelete: () -> Unit,
): List<List<MenuEntry>> =
    listOf(
        listOf(
            MenuEntry.Action("Play next", Icons.AutoMirrored.Rounded.PlaylistPlay, run = onPlayNext),
            MenuEntry.Action("Add to queue", Icons.AutoMirrored.Rounded.QueueMusic, run = onQueue),
        ),
        listOfNotNull(
            if (fills) null else MenuEntry.Action("Add songs", Icons.AutoMirrored.Rounded.PlaylistAdd, run = onAdd),
            onEditSearch?.let { MenuEntry.Action("Edit search", Icons.Rounded.Edit, run = it) },
            MenuEntry.Action("Rename", Icons.Rounded.Edit, run = onRename),
            MenuEntry.Action("Undo last edit", Icons.AutoMirrored.Rounded.Undo, run = onUndo),
        ),
        if (sync == null) emptyList()
        else
            listOf<MenuEntry>(MenuEntry.Toggle("Keep songs on this device", Icons.Rounded.DownloadForOffline, sync.kept, onKeep)) +
                if (sync.missing > 0u)
                    sync.devices.map { device ->
                        MenuEntry.Action(
                            "Fetch ${counted(sync.missing, "missing song")} from ${device.name}",
                            Icons.Rounded.CloudDownload,
                        ) {
                            onFetch(device)
                        }
                    }
                else emptyList(),
        if (link == null) emptyList()
        else
            listOf(
                MenuEntry.Toggle("Keep in step with ${link.service}", Icons.Rounded.Sync, link.keptInStep, onKeepInStep),
                MenuEntry.Action("Forget the link", Icons.Rounded.LinkOff, run = onForgetLink),
            ),
        listOf(MenuEntry.Action("Delete playlist", Icons.Rounded.Delete, destructive = true, run = onDelete)),
    )

@Composable
private fun PlaylistRow(
    entry: PlaylistEntry,
    last: Int,
    playing: Boolean,
    onPlay: () -> Unit,
    onMore: () -> Unit,
    onRemove: () -> Unit,
    onMove: (Int) -> Unit,
    selection: RowSelection,
    handle: @Composable () -> Unit,
) {
    val dismiss = rememberSwipeToDismissBoxState()
    LaunchedEffect(dismiss.currentValue) {
        if (dismiss.currentValue == SwipeToDismissBoxValue.EndToStart) {
            onRemove()
            delay(UNREMOVED_AFTER_MS)
            dismiss.reset()
        }
    }
    val title = entry.track.shownTitle
    SwipeToDismissBox(
        state = dismiss,
        enableDismissFromStartToEnd = false,
        backgroundContent = {
            Box(
                Modifier.fillMaxSize().background(MaterialTheme.colorScheme.errorContainer).padding(horizontal = 24.dp),
                contentAlignment = Alignment.CenterEnd,
            ) {
                Icon(Icons.Rounded.Delete, "Remove from playlist", tint = MaterialTheme.colorScheme.onErrorContainer)
            }
        },
        modifier =
            Modifier.reorderActions(
                title = title,
                removal = "playlist",
                onRemove = onRemove,
                onMoveUp = if (entry.position > 0) ({ onMove(entry.position - 1) }) else null,
                onMoveDown = if (entry.position < last) ({ onMove(entry.position + 1) }) else null,
            ),
    ) {
        Box(Modifier.background(MaterialTheme.colorScheme.surface)) {
            TrackRow(
                entry.track,
                onClick = onPlay,
                playing = playing,
                trailing = {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        IconButton(onClick = onMore) { Icon(Icons.Rounded.MoreVert, "More options") }
                        handle()
                    }
                },
                selection = selection,
            )
        }
    }
}

private const val UNREMOVED_AFTER_MS = 2_000L
