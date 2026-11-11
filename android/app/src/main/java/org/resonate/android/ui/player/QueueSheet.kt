package org.resonate.android.ui.player

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.PlaylistAdd
import androidx.compose.material.icons.automirrored.rounded.QueueMusic
import androidx.compose.material.icons.rounded.Delete
import androidx.compose.material.icons.rounded.Devices
import androidx.compose.material.icons.rounded.MoreVert
import androidx.compose.material.icons.rounded.MusicNote
import androidx.compose.material.icons.rounded.Timer
import androidx.compose.material.icons.rounded.TimerOff
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.SheetValue
import androidx.compose.material3.SwipeToDismissBox
import androidx.compose.material3.SwipeToDismissBoxValue
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberBottomSheetState
import androidx.compose.material3.rememberSwipeToDismissBoxState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import org.resonate.android.R
import org.resonate.android.nativecore.SleepTimer
import org.resonate.android.nativecore.Track
import org.resonate.android.ui.clock
import org.resonate.android.ui.components.CollectionRow
import org.resonate.android.ui.components.DragHandle
import org.resonate.android.ui.components.EmptyState
import org.resonate.android.ui.components.IconTile
import org.resonate.android.ui.components.LocalSheets
import org.resonate.android.ui.components.RowSelection
import org.resonate.android.ui.components.SectionHeader
import org.resonate.android.ui.components.SelectionRemoval
import org.resonate.android.ui.components.SheetRow
import org.resonate.android.ui.components.SongSelectionBar
import org.resonate.android.ui.components.TrackRow
import org.resonate.android.ui.components.rememberReorder
import org.resonate.android.ui.components.rememberSelection
import org.resonate.android.ui.components.reorderActions
import org.resonate.android.ui.components.reorderable
import org.resonate.android.ui.navigation.LocalNavigator
import org.resonate.android.ui.navigation.Screen
import org.resonate.android.ui.shownTitle
import org.resonate.android.words

@Composable
fun QueueSheet(model: PlayerViewModel, onDismiss: () -> Unit) {
    val queue by model.queue.collectAsStateWithLifecycle()
    val sheets = LocalSheets.current
    val navigator = LocalNavigator.current
    val expansion = LocalPlayerExpansion.current
    val scope = rememberCoroutineScope()
    val state = rememberBottomSheetState(SheetValue.Hidden, enabledValues = setOf(SheetValue.Hidden, SheetValue.Expanded))
    var devicesOpen by rememberSaveable { mutableStateOf(false) }
    var clearing by rememberSaveable { mutableStateOf(false) }
    val handoffRows = if (queue.handoffs.isNotEmpty()) 1 else 0
    val earlierRows = if (queue.history.isNotEmpty() && queue.earlier > 0) 1 else 0
    val historyRows = if (queue.history.isNotEmpty()) 1 + earlierRows + queue.history.size else 0
    val list = rememberLazyListState()
    val selection = rememberSelection<String>()
    val rows = remember(queue) { queue.history + listOfNotNull(queue.current) + queue.upNext }
    LaunchedEffect(rows) { selection.retain(rows.map { rowKey(it) }) }
    val currentRows = if (queue.current != null) 2 else 0
    val removeFromQueue = stringResource(R.string.queue_remove)
    val saveAsPlaylist = stringResource(R.string.queue_save_as_playlist)
    val reorder =
        rememberReorder(
            items = queue.upNext,
            list = list,
            keyOf = { rowKey(it) },
            firstIndex = { handoffRows + historyRows + currentRows + 1 },
            anchor = { queue.firstUpNext },
            onMove = { from, to -> model.move(queue.first + from, queue.first + to) },
        )
    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = state) {
        if (selection.active) {
            SongSelectionBar(
                tracks = selection.picked(rows) { rowKey(it) },
                onSelectAll = { selection.selectAll(rows.map { rowKey(it) }) },
                onClose = selection::clear,
                removal = SelectionRemoval(removeFromQueue, model::removeAll),
                windowInsets = WindowInsets(0),
            )
        } else Row(
            Modifier.fillMaxWidth().padding(start = 24.dp, end = 12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Column(Modifier.weight(1f)) {
                Text(
                    stringResource(R.string.queue_title),
                    style = MaterialTheme.typography.titleLarge,
                )
                if (queue.size > 0) {
                    Text(
                        pluralStringResource(R.plurals.count_songs, queue.size, queue.size),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
            if (queue.size > 0) {
                IconButton(
                    onClick = {
                        sheets.name(saveAsPlaylist) { model.saveAsPlaylist(it) }
                    }
                ) {
                    Icon(Icons.AutoMirrored.Rounded.PlaylistAdd, saveAsPlaylist)
                }
                TextButton(onClick = { clearing = true }) {
                    Text(stringResource(R.string.queue_clear))
                }
            }
        }
        if (queue.size == 0) {
            BoxWithConstraints(Modifier.fillMaxWidth().height(EMPTY_QUEUE_HEIGHT)) {
                val visible = maxHeight
                Column(
                    Modifier.fillMaxWidth()
                        .verticalScroll(rememberScrollState())
                        .heightIn(min = visible),
                    verticalArrangement = Arrangement.Center,
                ) {
                    EmptyState(
                        Icons.AutoMirrored.Rounded.QueueMusic,
                        stringResource(R.string.queue_empty),
                        detail = stringResource(R.string.queue_empty_detail),
                        action = stringResource(R.string.queue_browse_songs),
                        onAction = {
                            scope.launch { state.hide() }.invokeOnCompletion {
                                onDismiss()
                                expansion.collapse()
                                navigator.open(Screen.Library)
                            }
                        },
                    )
                }
            }
            return@ModalBottomSheet
        }
        LaunchedEffect(list) {
            snapshotFlow { list.layoutInfo.totalItemsCount }.first { it > 0 }
            list.scrollToItem(handoffRows + historyRows)
        }
        LazyColumn(state = list, modifier = Modifier.fillMaxHeight().navigationBarsPadding()) {
            if (queue.handoffs.isNotEmpty()) {
                item("handoffs") {
                    CollectionRow(
                        title = stringResource(R.string.queue_continue_listening),
                        supporting =
                            pluralStringResource(
                                R.plurals.queue_devices_available,
                                queue.handoffs.size,
                                queue.handoffs.size,
                            ),
                        onClick = { devicesOpen = true },
                        leading = { IconTile(Icons.Rounded.Devices) },
                    )
                }
            }
            if (queue.history.isNotEmpty()) {
                item("history") { SectionHeader(stringResource(R.string.queue_history)) }
                if (queue.earlier > 0) {
                    item("earlier") {
                        MoreRows(
                            pluralStringResource(R.plurals.queue_earlier, queue.earlier, queue.earlier)
                        ) {
                            model.widen(earlier = true)
                        }
                    }
                }
                items(queue.history, key = { rowKey(it) }) { track ->
                    val index = queue.first + queue.history.indexOf(track)
                    QueueRow(model, track, onClick = { model.jump(index) }, sheets = sheets, selection = selection.row(rowKey(track)))
                }
            }
            queue.current?.let { current ->
                item("now") { SectionHeader(stringResource(R.string.player_now_playing)) }
                item(rowKey(current)) {
                    TrackRow(
                        current,
                        playing = true,
                        onClick = {},
                        onMore = { sheets.song(current, queueId = current.queueId) },
                        selection = selection.row(rowKey(current)),
                    )
                }
            }
            item("next") { SectionHeader(stringResource(R.string.queue_up_next)) }
            val standing = (queue.history + listOfNotNull(queue.current)).map { rowKey(it) }.toSet()
            items(reorder.shown.filter { rowKey(it) !in standing }, key = { rowKey(it) }) { track ->
                val key = rowKey(track)
                val at = queue.upNext.indexOfFirst { rowKey(it) == key }
                val absolute = queue.first + queue.firstUpNext + at
                Box(reorderable(reorder, key)) {
                    QueueRow(
                        model,
                        track,
                        onClick = { if (at >= 0) model.jump(absolute) },
                        sheets = sheets,
                        selection = selection.row(key),
                        onMoveUp = if (at > 0) ({ model.move(absolute, absolute - 1) }) else null,
                        onMoveDown =
                            if (at in 0 until queue.upNext.size - 1) ({ model.move(absolute, absolute + 1) })
                            else null,
                        handle = { DragHandle(reorder, key, track.shownTitle) },
                    )
                }
            }
            if (queue.later > 0) {
                item("later") {
                    MoreRows(pluralStringResource(R.plurals.queue_more, queue.later, queue.later)) {
                        model.widen(earlier = false)
                    }
                }
            }
            item("end") { Spacer(Modifier.height(24.dp)) }
        }
    }
    if (clearing) {
        AlertDialog(
            onDismissRequest = { clearing = false },
            title = { Text(stringResource(R.string.queue_clear_title)) },
            text = { Text(stringResource(R.string.queue_clear_detail)) },
            confirmButton = {
                TextButton(
                    onClick = {
                        clearing = false
                        model.clear()
                    }
                ) {
                    Text(
                        stringResource(R.string.queue_clear_confirm),
                        color = MaterialTheme.colorScheme.error,
                    )
                }
            },
            dismissButton = {
                TextButton(onClick = { clearing = false }) {
                    Text(stringResource(R.string.queue_cancel))
                }
            },
        )
    }
    if (devicesOpen) {
        ModalBottomSheet(onDismissRequest = { devicesOpen = false }) {
            Text(
                stringResource(R.string.queue_continue_listening),
                style = MaterialTheme.typography.titleLarge,
                modifier = Modifier.padding(horizontal = 24.dp, vertical = 8.dp),
            )
            Column(Modifier.navigationBarsPadding().padding(bottom = 16.dp)) {
                queue.handoffs.forEach { handoff ->
                    CollectionRow(
                        title = handoff.device,
                        supporting =
                            pluralStringResource(
                                if (handoff.playing) R.plurals.queue_handoff_songs_playing
                                else R.plurals.count_songs,
                                handoff.songs.toInt(),
                                handoff.songs.toInt(),
                            ),
                        onClick = {
                            devicesOpen = false
                            model.continueFrom(handoff)
                        },
                        leading = { IconTile(Icons.Rounded.Devices) },
                    )
                }
            }
        }
    }
}

@Composable
private fun MoreRows(label: String, onClick: () -> Unit) {
    TextButton(onClick = onClick, modifier = Modifier.fillMaxWidth().padding(horizontal = 12.dp)) {
        Text(label)
    }
}

private const val UNREMOVED_AFTER_MS = 2_000L

private val EMPTY_QUEUE_HEIGHT = 360.dp

private fun rowKey(track: Track): String = "q${track.queueId ?: track.id}"

@Composable
private fun QueueRow(
    model: PlayerViewModel,
    track: Track,
    onClick: () -> Unit,
    sheets: org.resonate.android.ui.components.Sheets,
    selection: RowSelection,
    onMoveUp: (() -> Unit)? = null,
    onMoveDown: (() -> Unit)? = null,
    handle: (@Composable () -> Unit)? = null,
) {
    val dismiss = rememberSwipeToDismissBoxState()
    LaunchedEffect(dismiss.currentValue) {
        if (dismiss.currentValue == SwipeToDismissBoxValue.EndToStart) {
            model.remove(track)
            delay(UNREMOVED_AFTER_MS)
            dismiss.reset()
        }
    }
    SwipeToDismissBox(
        state = dismiss,
        backgroundContent = {
            Box(
                Modifier.fillMaxSize().background(MaterialTheme.colorScheme.errorContainer).padding(horizontal = 24.dp),
                contentAlignment = Alignment.CenterEnd,
            ) {
                Icon(
                    Icons.Rounded.Delete,
                    stringResource(R.string.queue_remove),
                    tint = MaterialTheme.colorScheme.onErrorContainer,
                )
            }
        },
        enableDismissFromStartToEnd = false,
        modifier =
            Modifier.reorderActions(
                words = words(),
                title = track.shownTitle,
                removal = R.string.reorder_remove_from_queue,
                onRemove = { model.remove(track) },
                onMoveUp = onMoveUp,
                onMoveDown = onMoveDown,
            ),
    ) {
        Box(Modifier.background(MaterialTheme.colorScheme.surfaceContainerLow)) {
            TrackRow(
                track,
                onClick = onClick,
                selection = selection,
                onMore = if (handle == null) ({ sheets.song(track, queueId = track.queueId) }) else null,
                trailing =
                    handle?.let { drag ->
                        {
                            Row(verticalAlignment = Alignment.CenterVertically) {
                                IconButton(onClick = { sheets.song(track, queueId = track.queueId) }) {
                                    Icon(
                                        Icons.Rounded.MoreVert,
                                        stringResource(R.string.queue_more_options),
                                    )

                                }
                                drag()
                            }
                        }
                    },
            )
        }
    }
}

@Composable
fun SleepTimerSheet(model: PlayerViewModel, onDismiss: () -> Unit) {
    val playing by model.playing.collectAsStateWithLifecycle()
    val remaining by model.sleepRemaining.collectAsStateWithLifecycle()
    ModalBottomSheet(onDismissRequest = onDismiss) {
        Column(Modifier.navigationBarsPadding().padding(bottom = 16.dp)) {
            Text(
                stringResource(R.string.player_sleep_timer),
                style = MaterialTheme.typography.titleLarge,
                modifier = Modifier.padding(horizontal = 24.dp),
            )
            remaining?.let {
                Text(
                    stringResource(R.string.sleep_pauses_in, clock(it)),
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(horizontal = 24.dp, vertical = 4.dp),
                )
            }
            Spacer(Modifier.height(8.dp))
            SLEEP_MINUTES.forEach { minutes ->
                SheetRow(
                    Icons.Rounded.Timer,
                    pluralStringResource(R.plurals.sleep_minutes, minutes, minutes),
                ) {
                    model.sleep(SleepTimer.After((minutes * 60).toUInt()))
                    onDismiss()
                }
            }
            SheetRow(Icons.Rounded.MusicNote, stringResource(R.string.sleep_end_of_song)) {
                model.sleep(SleepTimer.EndOfTrack)
                onDismiss()
            }
            SheetRow(
                Icons.AutoMirrored.Rounded.QueueMusic,
                stringResource(R.string.sleep_end_of_queue),
            ) {
                model.sleep(SleepTimer.EndOfQueue)
                onDismiss()
            }
            if (playing?.sleep != null) {
                SheetRow(Icons.Rounded.TimerOff, stringResource(R.string.sleep_turn_off)) {
                    model.sleep(null)
                    onDismiss()
                }
            }
        }
    }
}

private val SLEEP_MINUTES = listOf(15, 30, 45, 60, 90)
