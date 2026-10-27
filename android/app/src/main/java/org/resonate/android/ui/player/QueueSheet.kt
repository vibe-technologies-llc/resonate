package org.resonate.android.ui.player

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.PlaylistAdd
import androidx.compose.material.icons.automirrored.rounded.QueueMusic
import androidx.compose.material.icons.rounded.Delete
import androidx.compose.material.icons.rounded.MoreVert
import androidx.compose.material.icons.rounded.Devices
import androidx.compose.material.icons.rounded.Timer
import androidx.compose.material.icons.rounded.TimerOff
import androidx.compose.material.icons.rounded.MusicNote
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.SwipeToDismissBox
import androidx.compose.material3.SwipeToDismissBoxValue
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.SheetValue
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
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import org.resonate.android.nativecore.SleepTimer
import org.resonate.android.nativecore.Track
import org.resonate.android.ui.clock
import org.resonate.android.ui.components.CollectionRow
import org.resonate.android.ui.components.DragHandle
import org.resonate.android.ui.components.EmptyState
import org.resonate.android.ui.components.IconTile
import org.resonate.android.ui.components.LocalSheets
import org.resonate.android.ui.components.SectionHeader
import org.resonate.android.ui.components.SheetRow
import org.resonate.android.ui.components.TrackRow
import org.resonate.android.ui.components.rememberReorder
import org.resonate.android.ui.components.reorderable
import org.resonate.android.ui.counted
import org.resonate.android.ui.navigation.LocalNavigator
import org.resonate.android.ui.navigation.Screen

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
    val historyRows = if (queue.history.isNotEmpty()) 1 + queue.history.size else 0
    val list = rememberLazyListState()
    val currentRows = if (queue.current != null) 2 else 0
    val reorder =
        rememberReorder(
            items = queue.upNext,
            list = list,
            keyOf = { rowKey(it) },
            firstIndex = { handoffRows + historyRows + currentRows + 1 },
            anchor = { queue.firstUpNext },
            onMove = { from, to -> model.move(from, to) },
        )
    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = state) {
        Row(
            Modifier.fillMaxWidth().padding(start = 24.dp, end = 12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Column(Modifier.weight(1f)) {
                Text("Queue", style = MaterialTheme.typography.titleLarge)
                if (queue.size > 0) {
                    Text(
                        counted(queue.size, "song"),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
            if (queue.size > 0) {
                IconButton(
                    onClick = {
                        sheets.name("Save queue as playlist") { model.saveAsPlaylist(it) }
                    }
                ) {
                    Icon(Icons.AutoMirrored.Rounded.PlaylistAdd, "Save queue as playlist")
                }
                TextButton(onClick = { clearing = true }) { Text("Clear") }
            }
        }
        if (queue.size == 0) {
            Box(Modifier.fillMaxWidth().height(360.dp)) {
                EmptyState(
                    Icons.AutoMirrored.Rounded.QueueMusic,
                    "Your queue is empty",
                    detail = "Songs you play, play next or add to the queue appear here.",
                    action = "Browse songs",
                    onAction = {
                        scope.launch { state.hide() }.invokeOnCompletion {
                            onDismiss()
                            expansion.collapse()
                            navigator.open(Screen.Library)
                        }
                    },
                )
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
                        title = "Continue listening",
                        supporting = counted(queue.handoffs.size, "device") + " available",
                        onClick = { devicesOpen = true },
                        leading = { IconTile(Icons.Rounded.Devices) },
                    )
                }
            }
            if (queue.history.isNotEmpty()) {
                item("history") { SectionHeader("History") }
                items(queue.history, key = { rowKey(it) }) { track ->
                    val index = queue.history.indexOf(track)
                    QueueRow(model, track, onClick = { model.jump(index) }, sheets = sheets)
                }
            }
            queue.current?.let { current ->
                item("now") { SectionHeader("Now playing") }
                item(rowKey(current)) {
                    TrackRow(
                        current,
                        playing = true,
                        onClick = {},
                        onMore = { sheets.song(current, queueId = current.queueId) },
                    )
                }
            }
            item("next") { SectionHeader("Up next") }
            items(reorder.shown, key = { rowKey(it) }) { track ->
                val key = rowKey(track)
                Box(reorderable(reorder, key)) {
                    QueueRow(
                        model,
                        track,
                        onClick = {
                            val at = queue.upNext.indexOfFirst { rowKey(it) == key }
                            if (at >= 0) model.jump(queue.firstUpNext + at)
                        },
                        sheets = sheets,
                        handle = { DragHandle(reorder, key) },
                    )
                }
            }
            item("end") { Spacer(Modifier.height(24.dp)) }
        }
    }
    if (clearing) {
        AlertDialog(
            onDismissRequest = { clearing = false },
            title = { Text("Clear the queue?") },
            text = { Text("Songs up next and in your history are removed. The playing song keeps playing.") },
            confirmButton = {
                TextButton(
                    onClick = {
                        clearing = false
                        model.clear()
                    }
                ) {
                    Text("Clear queue", color = MaterialTheme.colorScheme.error)
                }
            },
            dismissButton = { TextButton(onClick = { clearing = false }) { Text("Cancel") } },
        )
    }
    if (devicesOpen) {
        ModalBottomSheet(onDismissRequest = { devicesOpen = false }) {
            Text(
                "Continue listening",
                style = MaterialTheme.typography.titleLarge,
                modifier = Modifier.padding(horizontal = 24.dp, vertical = 8.dp),
            )
            Column(Modifier.navigationBarsPadding().padding(bottom = 16.dp)) {
                queue.handoffs.forEach { handoff ->
                    CollectionRow(
                        title = handoff.device,
                        supporting =
                            counted(handoff.songs, "song") + if (handoff.playing) " · playing" else "",
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

private const val UNREMOVED_AFTER_MS = 2_000L

private fun rowKey(track: Track): String = "q${track.queueId ?: track.id}"

@Composable
private fun QueueRow(
    model: PlayerViewModel,
    track: Track,
    onClick: () -> Unit,
    sheets: org.resonate.android.ui.components.Sheets,
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
                Icon(Icons.Rounded.Delete, "Remove from queue", tint = MaterialTheme.colorScheme.onErrorContainer)
            }
        },
        enableDismissFromStartToEnd = false,
    ) {
        Box(Modifier.background(MaterialTheme.colorScheme.surfaceContainerLow)) {
            TrackRow(
                track,
                onClick = onClick,
                onMore = if (handle == null) ({ sheets.song(track, queueId = track.queueId) }) else null,
                trailing =
                    handle?.let { drag ->
                        {
                            Row(verticalAlignment = Alignment.CenterVertically) {
                                IconButton(onClick = { sheets.song(track, queueId = track.queueId) }) {
                                    Icon(Icons.Rounded.MoreVert, "More options")
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
                "Sleep timer",
                style = MaterialTheme.typography.titleLarge,
                modifier = Modifier.padding(horizontal = 24.dp),
            )
            remaining?.let {
                Text(
                    "Pauses in ${clock(it)}",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(horizontal = 24.dp, vertical = 4.dp),
                )
            }
            Spacer(Modifier.height(8.dp))
            SLEEP_MINUTES.forEach { minutes ->
                SheetRow(Icons.Rounded.Timer, "$minutes minutes") {
                    model.sleep(SleepTimer.After((minutes * 60).toUInt()))
                    onDismiss()
                }
            }
            SheetRow(Icons.Rounded.MusicNote, "End of song") {
                model.sleep(SleepTimer.EndOfTrack)
                onDismiss()
            }
            SheetRow(Icons.AutoMirrored.Rounded.QueueMusic, "End of queue") {
                model.sleep(SleepTimer.EndOfQueue)
                onDismiss()
            }
            if (playing?.sleep != null) {
                SheetRow(Icons.Rounded.TimerOff, "Turn off timer") {
                    model.sleep(null)
                    onDismiss()
                }
            }
        }
    }
}

private val SLEEP_MINUTES = listOf(15, 30, 45, 60, 90)
