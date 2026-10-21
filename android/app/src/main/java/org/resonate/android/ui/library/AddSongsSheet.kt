package org.resonate.android.ui.library

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Close
import androidx.compose.material.icons.rounded.Search
import androidx.compose.material.icons.rounded.SearchOff
import androidx.compose.material3.Button
import androidx.compose.material3.Checkbox
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.SheetValue
import androidx.compose.material3.rememberBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.testTagsAsResourceId
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.launch
import org.resonate.android.nativecore.Track
import org.resonate.android.ui.components.Artwork
import org.resonate.android.ui.components.EmptyState
import org.resonate.android.ui.components.Sizes
import org.resonate.android.ui.counted
import org.resonate.android.ui.shownArtist
import org.resonate.android.ui.shownTitle

@Composable
fun AddSongsSheet(
    catalog: StateFlow<List<Track>>,
    already: Set<ULong>,
    onAdd: (List<Track>) -> Unit,
    onDismiss: () -> Unit,
) {
    val songs by catalog.collectAsStateWithLifecycle()
    val state = rememberBottomSheetState(SheetValue.Hidden, enabledValues = setOf(SheetValue.Hidden, SheetValue.Expanded))
    val scope = rememberCoroutineScope()
    var query by rememberSaveable { mutableStateOf("") }
    val chosen = remember { mutableStateListOf<ULong>() }
    val matches =
        remember(songs, query) {
            songs.filter { matchesWords("${it.shownTitle} ${it.shownArtist} ${it.album}", query) }
        }
    val add: () -> Unit = {
        val byId = songs.associateBy { it.id }
        val picked = chosen.mapNotNull { byId[it] }
        scope.launch { state.hide() }.invokeOnCompletion {
            onAdd(picked)
            onDismiss()
        }
    }
    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = state) {
        Column(Modifier.fillMaxHeight(0.92f).navigationBarsPadding().imePadding().semantics { testTagsAsResourceId = true }.testTag("add-songs")) {
            Row(
                Modifier.fillMaxWidth().padding(start = 24.dp, end = 12.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Column(Modifier.weight(1f)) {
                    Text("Add songs", style = MaterialTheme.typography.titleLarge, modifier = Modifier.semantics { heading() })
                    Text(
                        if (chosen.isEmpty()) "Choose songs to add" else "${counted(chosen.size, "song")} chosen",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                if (chosen.isNotEmpty()) TextButton(onClick = { chosen.clear() }) { Text("Clear") }
            }
            OutlinedTextField(
                value = query,
                onValueChange = { query = it },
                placeholder = { Text("Search your songs") },
                leadingIcon = { Icon(Icons.Rounded.Search, contentDescription = null) },
                trailingIcon = {
                    if (query.isNotEmpty()) {
                        IconButton(onClick = { query = "" }) { Icon(Icons.Rounded.Close, "Clear search") }
                    }
                },
                singleLine = true,
                shape = CircleShape,
                modifier = Modifier.fillMaxWidth().padding(horizontal = Sizes.gutter, vertical = 12.dp).testTag("add-songs-search"),
            )
            if (matches.isEmpty()) {
                EmptyState(Icons.Rounded.SearchOff, "No songs match", modifier = Modifier.weight(1f), detail = "Try another word.")
            } else {
                LazyColumn(Modifier.weight(1f)) {
                    items(matches, key = { it.id.toString() }) { track ->
                        val picked = track.id in chosen
                        ListItem(
                            modifier =
                                Modifier.toggleable(value = picked, role = Role.Checkbox) { on ->
                                    if (on) chosen.add(track.id) else chosen.remove(track.id)
                                },
                            colors =
                                ListItemDefaults.colors(
                                    containerColor =
                                        if (picked) MaterialTheme.colorScheme.secondaryContainer else Color.Transparent
                                ),
                            leadingContent = { Artwork(track, Sizes.rowArt, shape = MaterialTheme.shapes.medium) },
                            supportingContent = {
                                Text(
                                    if (track.id in already) "${track.shownArtist} · already here" else track.shownArtist,
                                    maxLines = 1,
                                    overflow = TextOverflow.Ellipsis,
                                )
                            },
                            trailingContent = { Checkbox(checked = picked, onCheckedChange = null) },
                        ) {
                            Text(track.shownTitle, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        }
                    }
                }
            }
            Button(
                onClick = add,
                enabled = chosen.isNotEmpty(),
                modifier = Modifier.fillMaxWidth().padding(Sizes.gutter).testTag("add-chosen"),
            ) {
                Text(if (chosen.isEmpty()) "Add songs" else "Add ${counted(chosen.size, "song")}")
            }
        }
    }
}
