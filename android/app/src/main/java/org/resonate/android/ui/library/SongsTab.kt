package org.resonate.android.ui.library

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.ArrowDownward
import androidx.compose.material.icons.rounded.ArrowUpward
import androidx.compose.material.icons.rounded.ArrowDropDown
import androidx.compose.material.icons.rounded.Close
import androidx.compose.material.icons.rounded.FilterList
import androidx.compose.material.icons.rounded.SearchOff
import androidx.compose.material3.FilterChip
import androidx.compose.material3.FilterChipDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.InputChip
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.TextButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.resonate.android.nativecore.TrackOrder
import org.resonate.android.nativecore.defaultTrackFilter
import org.resonate.android.ui.LocalResonate
import org.resonate.android.ui.components.EmptyState
import org.resonate.android.ui.components.LocalSheets
import org.resonate.android.ui.components.PlayShuffleButtons
import org.resonate.android.ui.components.Sizes
import org.resonate.android.ui.components.TrackRow
import org.resonate.android.ui.counted
import org.resonate.android.ui.length

@Composable
fun SongsTab(model: LibraryViewModel, padding: PaddingValues) {
    val resonate = LocalResonate.current
    val sheets = LocalSheets.current
    val songs by model.songs.collectAsStateWithLifecycle()
    val chosen by model.filter.collectAsStateWithLifecycle()
    val playing by model.playing.collectAsStateWithLifecycle()
    val filter = chosen ?: defaultTrackFilter()
    var filtering by rememberSaveable { mutableStateOf(false) }
    val list = rememberLazyListState()
    val totalLength =
        remember(songs) { songs.takeIf { it.isNotEmpty() }?.let { rows -> length(rows.sumOf { it.durationMs }) } }
    Box(Modifier.fillMaxSize()) {
        LazyColumn(
            state = list,
            contentPadding = padding,
            modifier = Modifier.fillMaxSize().testTag("songs"),
        ) {
            if (songs.isNotEmpty()) {
                item("play") {
                    PlayShuffleButtons(
                        onPlay = { resonate.playInOrder(songs) },
                        onShuffle = { resonate.playShuffled(songs) },
                        modifier = Modifier.padding(top = 12.dp),
                    )
                }
            }
            item("chips") {
                FilterChips(
                    title = counted(songs.size, "song"),
                    detail = totalLength,
                    filter = filter,
                    onOrder = { order -> model.change { it.copy(order = order) } },
                    onDescending = { descending -> model.change { it.copy(descending = descending) } },
                    onFilters = { filtering = true },
                    onRemove = { part -> model.change { it.without(part) } },
                    onClear = model::clearFilter,
                )
            }
            if (songs.isEmpty()) {
                item("none") {
                    EmptyState(
                        Icons.Rounded.SearchOff,
                        "No songs match",
                        modifier = Modifier.padding(top = 24.dp),
                        detail = "Try loosening or clearing your filters.",
                        action = "Clear filters",
                        onAction = model::clearFilter,
                    )
                }
            }
            itemsIndexed(songs, key = { _, track -> track.id.toString() }) { index, track ->
                TrackRow(
                    track,
                    onClick = { resonate.playback.play(songs, index) },
                    playing = track.id == playing,
                    onMore = { sheets.song(track) },
                )
            }
        }
        FastScroller(list, songs.size, Modifier.align(Alignment.TopEnd).padding(padding))
    }
    if (filtering) {
        FilterSheet(model, filter, onDismiss = { filtering = false })
    }
}

@Composable
private fun FilterChips(
    title: String,
    detail: String?,
    filter: org.resonate.android.nativecore.TrackFilter,
    onOrder: (TrackOrder) -> Unit,
    onDescending: (Boolean) -> Unit,
    onFilters: () -> Unit,
    onRemove: (FilterPart) -> Unit,
    onClear: () -> Unit,
) {
    val parts = filter.parts()
    var sorting by remember { mutableStateOf(false) }
    Column {
        Row(
            Modifier.fillMaxWidth().padding(start = Sizes.gutter, end = Sizes.gutter, top = 8.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Column(Modifier.weight(1f)) {
                Text(title, style = MaterialTheme.typography.titleMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
                if (detail != null) {
                    Text(
                        detail,
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
            }
            Box {
                FilterChip(
                    selected = filter.ordered,
                    onClick = { sorting = true },
                    label = { Text(filter.orderLabel(), maxLines = 1) },
                    leadingIcon = {
                        Icon(
                            if (filter.descending) Icons.Rounded.ArrowDownward
                            else Icons.Rounded.ArrowUpward,
                            null,
                            Modifier.size(FilterChipDefaults.IconSize),
                        )
                    },
                    trailingIcon = {
                        Icon(Icons.Rounded.ArrowDropDown, null, Modifier.size(FilterChipDefaults.IconSize))
                    },
                    modifier = Modifier.testTag("sort-songs"),
                )
                GroupedMenu(
                    sorting,
                    { sorting = false },
                    listOf(
                        TrackOrder.entries.map { order ->
                            MenuEntry.Choice(order.label, order == filter.order) {
                                if (order == filter.order) onDescending(!filter.descending)
                                else onOrder(order)
                            }
                        }
                    ),
                )
            }
            FilterChip(
                selected = parts.isNotEmpty(),
                onClick = onFilters,
                label = { Text(if (parts.isEmpty()) "Filters" else "Filters · ${parts.size}", maxLines = 1) },
                leadingIcon = {
                    Icon(Icons.Rounded.FilterList, null, Modifier.size(FilterChipDefaults.IconSize))
                },
                modifier = Modifier.testTag("filter-songs"),
            )
        }
        if (parts.isNotEmpty() || filter.ordered) {
            LazyRow(
                contentPadding = PaddingValues(horizontal = Sizes.gutter, vertical = 4.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                parts.forEach { part ->
                    item(part.name) {
                        InputChip(
                            selected = true,
                            onClick = { onRemove(part) },
                            label = { Text(filter.label(part), maxLines = 1, overflow = TextOverflow.Ellipsis) },
                            trailingIcon = {
                                Icon(
                                    Icons.Rounded.Close,
                                    "Remove ${filter.label(part)}",
                                    Modifier.size(FilterChipDefaults.IconSize),
                                )
                            },
                        )
                    }
                }
                item("clear") { TextButton(onClick = onClear) { Text("Clear") } }
            }
        }
    }
}
