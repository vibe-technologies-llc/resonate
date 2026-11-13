package org.resonate.android.ui.library

import androidx.activity.compose.LocalActivity
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.PlaylistAdd
import androidx.compose.material.icons.automirrored.rounded.PlaylistPlay
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
import androidx.compose.material.icons.rounded.Share
import androidx.compose.material.icons.rounded.Sync
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CircularProgressIndicator
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
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.lifecycleScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import org.resonate.android.LinkedPlaylistWords
import org.resonate.android.R
import org.resonate.android.TogetherWords
import org.resonate.android.Words
import org.resonate.android.nativecore.PlaylistLinkInfo
import org.resonate.android.ui.LocalResonate
import org.resonate.android.ui.components.CollectionRow
import org.resonate.android.ui.components.DragHandle
import org.resonate.android.ui.components.IconTile
import org.resonate.android.ui.components.LocalSheets
import org.resonate.android.ui.components.PlaylistCover
import org.resonate.android.ui.components.RowSelection
import org.resonate.android.ui.components.SelectionRemoval
import org.resonate.android.ui.components.SongSelectionBar
import org.resonate.android.ui.components.TrackRow
import org.resonate.android.ui.components.rememberArtwork
import org.resonate.android.ui.components.rememberReorder
import org.resonate.android.ui.components.rememberSelection
import org.resonate.android.ui.components.reorderActions
import org.resonate.android.ui.components.reorderable
import org.resonate.android.ui.navigation.LocalNavigator
import org.resonate.android.ui.navigation.Screen
import org.resonate.android.ui.resonateViewModel
import org.resonate.android.ui.shownTitle
import org.resonate.android.ui.theme.ArtworkTheme
import org.resonate.android.ui.theme.rememberSeedOf
import org.resonate.android.ui.together.SharingSheet
import org.resonate.android.words

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
    var sharing by rememberSaveable { mutableStateOf(false) }
    val shared by model.shared.collectAsStateWithLifecycle()
    val followed = playlist?.followed == true
    val mayEdit = TogetherWords.mayEdit(shared, followed)
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
    val words = words()
    val renameTitle = stringResource(R.string.library_playlist_rename)
    val editSearchTitle = stringResource(R.string.library_playlist_edit_search_title)
    val deleted = stringResource(R.string.library_playlist_deleted, name)
    val undo = stringResource(R.string.library_undo)
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
                                if (search != null || !mayEdit) null
                                else
                                    SelectionRemoval(stringResource(R.string.library_playlist_remove)) {
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
                        words,
                        sync.takeIf { search == null },
                        fills = search != null,
                        mayEdit = mayEdit,
                        onShare = if (search == null) ({ sharing = true }) else null,
                        inShare = shared != null,
                        onPlayNext = { resonate.playNext(tracks) },
                        onQueue = { resonate.addToQueue(tracks) },
                        onAdd = { adding = true },
                        onRename = { sheets.name(renameTitle, name) { model.rename(it) } },
                        onEditSearch =
                            search?.let { held ->
                                {
                                    sheets.editSearch(editSearchTitle, SavedSearchDraft.of(name, held)) { renamed, revised ->
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
                    description = stringResource(R.string.library_playlist_options),
                )
            },
        ) { rows ->
            item("header") {
                Column {
                    CollectionHeader(
                        title = name,
                        overline =
                            when {
                                search != null -> stringResource(R.string.library_saved_search)
                                shared == null && followed -> stringResource(R.string.library_overline_followed)
                                shared == null && playlist?.shared == true -> stringResource(R.string.library_overline_shared)
                                shared == null -> stringResource(R.string.library_overline_playlist)
                                mayEdit -> stringResource(R.string.library_overline_shared)
                                else -> stringResource(R.string.library_overline_followed)
                            },
                        summary = collectionSummary(words, tracks = rows.map { it.track }),
                        onPlay = { resonate.playInOrder(rows.map { it.track }) },
                        onShuffle = { resonate.playShuffled(rows.map { it.track }) },
                        playable = rows.isNotEmpty(),
                        extra =
                            when {
                                search != null -> { align ->
                                    SavedSearchLines(search.text, search.reading(words), align)
                                }
                                link != null -> { align ->
                                    LinkedPlaylistLine(
                                        link = link,
                                        refreshing = refreshing,
                                        online = online,
                                        align = align,
                                        onRefresh = model::refreshLink,
                                    )
                                }
                                else -> null
                            },
                    ) { side ->
                        PlaylistCover(playlist?.covers.orEmpty(), side, shape = MaterialTheme.shapes.extraLarge)
                    }
                    if (search == null && mayEdit && rows.isNotEmpty()) {
                        CollectionRow(
                            title = stringResource(R.string.library_add_songs),
                            supporting = null,
                            onClick = { adding = true },
                            leading = { IconTile(Icons.AutoMirrored.Rounded.PlaylistAdd) },
                            modifier = Modifier.testTag("add-songs-button"),
                        )
                    }
                }
            }
            if (search != null || !mayEdit) {
                if (rows.isEmpty()) {
                    item("empty") {
                        EmptyCollection(
                            detail =
                                if (search != null) stringResource(R.string.library_playlist_search_matches_nothing)
                                else stringResource(R.string.library_playlist_nothing_held),
                        )
                    }
                }
                items(rows.size, key = { rows[it].key }) { at ->
                    val entry = rows[at]
                    TrackRow(
                        entry.track,
                        onClick = { resonate.playback.play(tracks, entry.position) },
                        playing = entry.track.id == playing,
                        trailing = {
                            IconButton(onClick = { sheets.song(entry.track) }) {
                                Icon(Icons.Rounded.MoreVert, stringResource(R.string.library_more_options))
                            }
                        },
                        selection = selection.row(entry.key),
                    )
                }
                return@CollectionPage
            }
            if (rows.isEmpty()) {
                item("empty") {
                    EmptyCollection(
                        detail = stringResource(R.string.library_playlist_shape),
                        action = {
                            Button(onClick = { adding = true }, modifier = Modifier.testTag("add-songs-button")) {
                                AddSongsLabel()
                            }
                        },
                    )
                }
            }
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
    if (sharing) SharingSheet(screen.id, name, sharedHere = playlist?.shared == true) { sharing = false }
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
            title = { Text(stringResource(R.string.library_playlist_delete_title, name)) },
            text = { Text(stringResource(R.string.library_playlist_delete_detail)) },
            confirmButton = {
                TextButton(
                    onClick = {
                        deleting = false
                        model.delete()
                        navigator.back()
                        lasting.launch {
                            val answer =
                                sheets.snackbar.showSnackbar(
                                    deleted,
                                    actionLabel = undo,
                                    duration = SnackbarDuration.Long,
                                )
                            if (answer == SnackbarResult.ActionPerformed) model.undoDeletion()
                        }
                    }
                ) {
                    Text(stringResource(R.string.library_delete), color = MaterialTheme.colorScheme.error)
                }
            },
            dismissButton = {
                TextButton(onClick = { deleting = false }) { Text(stringResource(R.string.library_cancel)) }
            },
        )
    }
}

@Composable
private fun SavedSearchLines(text: String, reading: String, align: TextAlign) {
    Column(Modifier.fillMaxWidth()) {
        Text(
            text.ifBlank { stringResource(R.string.library_playlist_every_song) },
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            textAlign = align,
            modifier = Modifier.fillMaxWidth().testTag("saved-search-text"),
        )
        Text(
            reading,
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            textAlign = align,
            modifier = Modifier.fillMaxWidth().testTag("saved-search-reading"),
        )
    }
}

@Composable
private fun LinkedPlaylistLine(
    link: PlaylistLinkInfo,
    refreshing: Boolean,
    online: Boolean,
    align: TextAlign,
    onRefresh: () -> Unit,
) {
    val refreshingFrom = stringResource(R.string.library_playlist_refreshing_from, link.service)
    Row(verticalAlignment = Alignment.CenterVertically) {
        Text(
            LinkedPlaylistWords.line(words(), link),
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            textAlign = align,
            modifier = Modifier.weight(1f, fill = false).testTag("playlist-link-line"),
        )
        Spacer(Modifier.width(8.dp))
        Box(Modifier.size(48.dp), contentAlignment = Alignment.Center) {
            if (refreshing) {
                CircularProgressIndicator(
                    Modifier.size(24.dp).semantics { contentDescription = refreshingFrom }
                )
            } else {
                IconButton(
                    onClick = onRefresh,
                    enabled = online,
                    modifier = Modifier.testTag("playlist-link-refresh"),
                ) {
                    Icon(
                        Icons.Rounded.Refresh,
                        contentDescription = stringResource(R.string.library_playlist_refresh_from, link.service),
                    )
                }
            }
        }
    }
}

@Composable
private fun AddSongsLabel() {
    Icon(Icons.AutoMirrored.Rounded.PlaylistAdd, null, Modifier.size(ButtonDefaults.IconSize))
    Spacer(Modifier.width(ButtonDefaults.IconSpacing))
    Text(stringResource(R.string.library_add_songs))
}

private fun playlistMenu(
    words: Words,
    sync: PlaylistSyncView?,
    fills: Boolean,
    mayEdit: Boolean,
    onShare: (() -> Unit)?,
    inShare: Boolean,
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
            MenuEntry.Action(
                words.string(R.string.library_play_next),
                Icons.AutoMirrored.Rounded.PlaylistPlay,
                run = onPlayNext,
            ),
            MenuEntry.Action(
                words.string(R.string.library_add_to_queue),
                Icons.AutoMirrored.Rounded.QueueMusic,
                run = onQueue,
            ),
        ),
        listOfNotNull(
            onShare?.let { MenuEntry.Action(words.string(R.string.library_share), Icons.Rounded.Share, run = it) }
        ),
        if (!mayEdit) emptyList()
        else
            listOfNotNull(
                if (fills) null
                else
                    MenuEntry.Action(
                        words.string(R.string.library_add_songs),
                        Icons.AutoMirrored.Rounded.PlaylistAdd,
                        run = onAdd,
                    ),
                onEditSearch?.let {
                    MenuEntry.Action(words.string(R.string.library_playlist_edit_search), Icons.Rounded.Edit, run = it)
                },
                MenuEntry.Action(
                    words.string(R.string.library_playlist_rename_short),
                    Icons.Rounded.Edit,
                    run = onRename,
                ),
                MenuEntry.Action(
                    words.string(R.string.library_playlist_undo_edit),
                    Icons.AutoMirrored.Rounded.Undo,
                    run = onUndo,
                ),
            ),
        if (sync == null) emptyList()
        else
            listOf<MenuEntry>(
                MenuEntry.Toggle(
                    words.string(R.string.library_playlist_keep_on_device),
                    Icons.Rounded.DownloadForOffline,
                    sync.kept,
                    onKeep,
                )
            ) +
                if (sync.missing > 0u)
                    sync.devices.map { device ->
                        MenuEntry.Action(
                            words.plural(
                                R.plurals.library_playlist_fetch,
                                sync.missing.toInt(),
                                sync.missing.toInt(),
                                device.name,
                            ),
                            Icons.Rounded.CloudDownload,
                        ) {
                            onFetch(device)
                        }
                    }
                else emptyList(),
        if (link == null) emptyList()
        else
            listOf(
                MenuEntry.Toggle(
                    words.string(R.string.library_playlist_keep_in_step, link.service),
                    Icons.Rounded.Sync,
                    link.keptInStep,
                    onKeepInStep,
                ),
                MenuEntry.Action(
                    words.string(R.string.library_playlist_forget_link),
                    Icons.Rounded.LinkOff,
                    run = onForgetLink,
                ),
            ),
        if (inShare) emptyList()
        else
            listOf(
                MenuEntry.Action(
                    words.string(R.string.library_playlist_delete),
                    Icons.Rounded.Delete,
                    destructive = true,
                    run = onDelete,
                )
            ),
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
    val words = words()
    SwipeToDismissBox(
        state = dismiss,
        enableDismissFromStartToEnd = false,
        backgroundContent = {
            Box(
                Modifier.fillMaxSize().background(MaterialTheme.colorScheme.errorContainer).padding(horizontal = 24.dp),
                contentAlignment = Alignment.CenterEnd,
            ) {
                Icon(
                    Icons.Rounded.Delete,
                    stringResource(R.string.library_playlist_remove),
                    tint = MaterialTheme.colorScheme.onErrorContainer,
                )
            }
        },
        modifier =
            Modifier.reorderActions(
                words = words,
                title = title,
                removal = R.string.reorder_remove_from_playlist,
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
                        IconButton(onClick = onMore) {
                            Icon(Icons.Rounded.MoreVert, stringResource(R.string.library_more_options))
                        }
                        handle()
                    }
                },
                selection = selection,
            )
        }
    }
}

private const val UNREMOVED_AFTER_MS = 2_000L
