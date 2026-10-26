package org.resonate.android.ui.library

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.toggleable
import androidx.compose.ui.semantics.Role
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Check
import androidx.compose.material.icons.rounded.Favorite
import androidx.compose.material.icons.rounded.FiberNew
import androidx.compose.material.icons.rounded.SwapVert
import androidx.compose.material3.Button
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.ui.window.PopupProperties
import androidx.compose.material3.ExposedDropdownMenuAnchorType
import androidx.compose.material3.ExposedDropdownMenuBox
import androidx.compose.material3.FilterChip
import androidx.compose.material3.FilterChipDefaults
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.SheetValue
import androidx.compose.material3.rememberBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.testTagsAsResourceId
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.resonate.android.nativecore.TrackFilter
import org.resonate.android.nativecore.TrackOrder
import org.resonate.android.ui.LocalResonate
import org.resonate.android.ui.albumOwner
import org.resonate.android.ui.components.Sizes

@OptIn(ExperimentalLayoutApi::class)
@Composable
fun FilterSheet(model: LibraryViewModel, initial: TrackFilter, onDismiss: () -> Unit) {
    val state = rememberBottomSheetState(SheetValue.Hidden, enabledValues = setOf(SheetValue.Hidden, SheetValue.Expanded))
    val scope = rememberCoroutineScope()
    val genres by model.genres.collectAsStateWithLifecycle()
    var draft by remember { mutableStateOf(initial) }
    val close: (() -> Unit) -> Unit = { then ->
        scope.launch { state.hide() }.invokeOnCompletion {
            then()
            onDismiss()
        }
    }
    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = state) {
        Column(
            Modifier.navigationBarsPadding()
                .imePadding()
                .semantics { testTagsAsResourceId = true }
                .testTag("filter-sheet")
        ) {
            Column(Modifier.weight(1f, fill = false).verticalScroll(rememberScrollState())) {
                Text(
                    "Filter and sort",
                    style = MaterialTheme.typography.titleLarge,
                    modifier = Modifier.padding(horizontal = 24.dp).semantics { heading() },
                )
                SheetSection("Sort by")
                ChoiceChips(
                    TrackOrder.entries,
                    selected = draft.order,
                    label = { it.label },
                    onSelect = { draft = draft.copy(order = it) },
                )
                SwitchRow(Icons.Rounded.SwapVert, "Descending order", draft.descending) {
                    draft = draft.copy(descending = it)
                }
                HorizontalDivider(Modifier.padding(horizontal = Sizes.gutter, vertical = 8.dp))
                SwitchRow(Icons.Rounded.Favorite, "Favourites only", draft.favourites) {
                    draft = draft.copy(favourites = it)
                }
                SwitchRow(Icons.Rounded.FiberNew, "Never played", draft.unplayed) {
                    draft = draft.copy(unplayed = it)
                }
                SheetSection("From")
                Column(
                    Modifier.padding(horizontal = Sizes.gutter),
                    verticalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    SuggestingField("Artist", draft.artist, { songs -> songs.map { it.artist }.asSequence() + songs.map { it.albumOwner }.asSequence() }) {
                        draft = draft.copy(artist = it)
                    }
                    SuggestingField("Album", draft.album, { songs -> songs.map { it.album }.asSequence() }) {
                        draft = draft.copy(album = it)
                    }
                    if (genres.size > GENRE_CHIPS_AT_MOST) {
                        SuggestingField("Genre", draft.genre, { songs -> songs.map { it.genre }.asSequence() }) {
                            draft = draft.copy(genre = it)
                        }
                    }
                }
                if (genres.isNotEmpty() && genres.size <= GENRE_CHIPS_AT_MOST) {
                    SheetSection("Genre")
                    ChoiceChips(
                        genres,
                        selected = genres.firstOrNull { it.equals(draft.genre.trim(), ignoreCase = true) },
                        label = { it },
                        onSelect = { genre -> draft = draft.copy(genre = if (draft.genre == genre) "" else genre) },
                    )
                }
                SheetSection("Minimum sample rate")
                ChoiceChips(sampleRates, draft.minimumRate, ::rateLabel) { draft = draft.copy(minimumRate = it) }
                SheetSection("Minimum bit depth")
                ChoiceChips(bitDepths, draft.minimumBits, ::depthLabel) { draft = draft.copy(minimumBits = it) }
                Spacer(Modifier.height(16.dp))
            }
            HorizontalDivider()
            Row(
                Modifier.fillMaxWidth().padding(horizontal = Sizes.gutter, vertical = 8.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp, androidx.compose.ui.Alignment.End),
            ) {
                TextButton(onClick = { close(model::clearFilter) }) { Text("Clear all") }
                Button(
                    onClick = { close { model.applyFilter(draft) } },
                    modifier = Modifier.testTag("apply-filters"),
                ) {
                    Text("Show songs")
                }
            }
        }
    }
}

@Composable
private fun SheetSection(title: String) {
    Text(
        title,
        style = MaterialTheme.typography.titleSmall,
        color = MaterialTheme.colorScheme.primary,
        modifier = Modifier.padding(start = 24.dp, end = 24.dp, top = 20.dp, bottom = 8.dp).semantics { heading() },
    )
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun <T> ChoiceChips(values: List<T>, selected: T?, label: (T) -> String, onSelect: (T) -> Unit) {
    FlowRow(
        Modifier.fillMaxWidth().padding(horizontal = Sizes.gutter),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        values.forEach { value ->
            val chosen = value == selected
            FilterChip(
                selected = chosen,
                onClick = { onSelect(value) },
                label = { Text(label(value)) },
                leadingIcon =
                    if (chosen) {
                        { Icon(Icons.Rounded.Check, null, Modifier.size(FilterChipDefaults.IconSize)) }
                    } else null,
            )
        }
    }
}

@Composable
private fun SwitchRow(icon: ImageVector, label: String, checked: Boolean, onChange: (Boolean) -> Unit) {
    ListItem(
        modifier = Modifier.toggleable(value = checked, role = Role.Switch, onValueChange = onChange).padding(horizontal = 8.dp),
        colors = ListItemDefaults.colors(containerColor = Color.Transparent),
        leadingContent = { Icon(icon, contentDescription = null) },
        trailingContent = { Switch(checked = checked, onCheckedChange = null) },
    ) {
        Text(label)
    }
}

@Composable
private fun SuggestingField(
    label: String,
    value: String,
    values: (List<org.resonate.android.nativecore.Track>) -> Sequence<String>,
    onChange: (String) -> Unit,
) {
    val catalog by LocalResonate.current.library.allTracks.collectAsStateWithLifecycle()
    var open by remember { mutableStateOf(false) }
    val typed = value
    val offered by
        produceState(emptyList<String>(), catalog, typed) {
            this.value = withContext(Dispatchers.Default) { suggestions(values(catalog), typed) }
        }
    ExposedDropdownMenuBox(expanded = open && offered.isNotEmpty(), onExpandedChange = { open = it }) {
        OutlinedTextField(
            value = value,
            onValueChange = {
                onChange(it)
                open = true
            },
            label = { Text(label) },
            singleLine = true,
            keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done),
            modifier = Modifier.fillMaxWidth().menuAnchor(ExposedDropdownMenuAnchorType.PrimaryEditable),
        )
        DropdownMenu(
            expanded = open && offered.isNotEmpty(),
            onDismissRequest = { open = false },
            properties = PopupProperties(focusable = false),
            modifier = Modifier.exposedDropdownSize(),
        ) {
            offered.forEach { suggestion ->
                DropdownMenuItem(
                    text = { Text(suggestion) },
                    onClick = {
                        onChange(suggestion)
                        open = false
                    },
                )
            }
        }
    }
}

private const val GENRE_CHIPS_AT_MOST = 24
