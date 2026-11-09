package org.resonate.android.ui.equaliser

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Add
import androidx.compose.material.icons.rounded.ArrowDropDown
import androidx.compose.material.icons.rounded.Check
import androidx.compose.material.icons.rounded.DeleteOutline
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import org.resonate.android.nativecore.EqualiserBand
import org.resonate.android.nativecore.EqualiserCurve
import org.resonate.android.nativecore.EqualiserFilter
import org.resonate.android.ui.components.Sizes

@Composable
fun BandChips(
    curve: EqualiserCurve,
    chosen: Int?,
    bounds: EqualiserBounds,
    onSelect: (Int) -> Unit,
    onAdd: () -> Unit,
) {
    val state = rememberLazyListState(initialFirstVisibleItemIndex = chosen ?: 0)
    LaunchedEffect(chosen, curve.bands.size) {
        val wanted = chosen ?: return@LaunchedEffect
        val seen = state.layoutInfo.visibleItemsInfo.map { it.index }
        if (wanted !in seen) state.animateScrollToItem(wanted)
    }
    Row(
        Modifier.fillMaxWidth().padding(end = Sizes.gutter, top = 8.dp, bottom = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        LazyRow(
            state = state,
            contentPadding = PaddingValues(start = Sizes.gutter, end = 8.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            modifier = Modifier.weight(1f).testTag("band-chips"),
        ) {
            itemsIndexed(curve.bands) { index, band ->
                FilterChip(
                    selected = chosen == index,
                    onClick = { onSelect(index) },
                    label = { Text("${index + 1}") },
                    modifier =
                        Modifier.semantics {
                                contentDescription = "Band ${index + 1}, ${bandName(band)}"
                            }
                            .testTag("band-chip-$index"),
                )
            }
        }
        FilledTonalButton(
            onClick = onAdd,
            enabled = curve.bands.size < bounds.bandsAtMost,
            contentPadding = ButtonDefaults.ButtonWithIconContentPadding,
            modifier = Modifier.testTag("add-band"),
        ) {
            Icon(Icons.Rounded.Add, null, Modifier.size(ButtonDefaults.IconSize))
            Spacer(Modifier.width(ButtonDefaults.IconSpacing))
            Text("Add band")
        }
    }
}

@Composable
fun BandCard(
    band: EqualiserBand,
    index: Int,
    bounds: EqualiserBounds,
    onChange: (EqualiserBand) -> Unit,
    onRemove: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val number = index + 1
    EqualiserCard(modifier.testTag("band-card")) {
        Column(Modifier.padding(start = 16.dp, end = 8.dp, top = 4.dp, bottom = 12.dp)) {
            FlowRow(
                Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                itemVerticalAlignment = Alignment.CenterVertically,
            ) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(
                        "Band $number",
                        style = MaterialTheme.typography.titleMedium,
                        modifier = Modifier.semantics { heading() },
                    )
                    Text(
                        " · ",
                        style = MaterialTheme.typography.titleMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    FilterMenu(band.filter, number) { onChange(band.copy(filter = it)) }
                }
                TextButton(
                    onClick = onRemove,
                    colors =
                        ButtonDefaults.textButtonColors(
                            contentColor = MaterialTheme.colorScheme.error
                        ),
                    contentPadding = ButtonDefaults.TextButtonWithIconContentPadding,
                    modifier =
                        Modifier.semantics { contentDescription = "Remove band $number" }
                            .testTag("remove-band"),
                ) {
                    Icon(Icons.Rounded.DeleteOutline, null, Modifier.size(ButtonDefaults.IconSize))
                    Spacer(Modifier.width(ButtonDefaults.IconSpacing))
                    Text("Remove")
                }
            }
            Column(Modifier.padding(end = 8.dp)) {
                MeasureControl(
                    measure = gainMeasure(bounds),
                    value = band.gainDb,
                    description = "Band $number gain",
                    onCommit = { onChange(band.copy(gainDb = tidyDb(it.coerceIn(bounds.gain)))) },
                    tag = "band-gain",
                    enabled = band.filter.usesGain,
                    unused = "Not used by ${band.filter.label.lowercase()}",
                )
                MeasureControl(
                    measure = hertzMeasure(bounds),
                    value = band.frequencyHz,
                    description = "Band $number frequency",
                    onCommit = {
                        onChange(band.copy(frequencyHz = tidyHertz(it.coerceIn(bounds.hertz))))
                    },
                    tag = "band-frequency",
                )
                MeasureControl(
                    measure = widthMeasure(bounds),
                    value = band.q,
                    description = "Band $number width",
                    onCommit = { onChange(band.copy(q = tidyQ(it.coerceIn(bounds.q)))) },
                    tag = "band-width",
                )
            }
        }
    }
}

@Composable
private fun FilterMenu(filter: EqualiserFilter, number: Int, onChoose: (EqualiserFilter) -> Unit) {
    var open by remember { mutableStateOf(false) }
    Box {
        TextButton(
            onClick = { open = true },
            contentPadding = PaddingValues(start = 8.dp, end = 4.dp),
            modifier =
                Modifier.semantics { contentDescription = "Band $number type, ${filter.label}" }
                    .testTag("band-type"),
        ) {
            Text(filter.label, style = MaterialTheme.typography.titleMedium)
            Icon(Icons.Rounded.ArrowDropDown, null)
        }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            EqualiserFilter.entries.forEach { choice ->
                DropdownMenuItem(
                    text = { Text(choice.label) },
                    onClick = {
                        open = false
                        if (choice != filter) onChoose(choice)
                    },
                    trailingIcon =
                        if (choice == filter) {
                            { Icon(Icons.Rounded.Check, null) }
                        } else null,
                    modifier = Modifier.testTag("band-type-${choice.name.lowercase()}"),
                )
            }
        }
    }
}
