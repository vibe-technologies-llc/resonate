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
import androidx.compose.material3.AssistChip
import androidx.compose.material3.AssistChipDefaults
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
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
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.graphics.BlendMode
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.CompositingStrategy
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import org.resonate.android.R
import org.resonate.android.nativecore.EqualiserBand
import org.resonate.android.nativecore.EqualiserCurve
import org.resonate.android.nativecore.EqualiserFilter
import org.resonate.android.ui.components.Sizes
import org.resonate.android.words

@Composable
fun BandChips(
    curve: EqualiserCurve,
    chosen: Int?,
    bounds: EqualiserBounds,
    onSelect: (Int) -> Unit,
    onAdd: () -> Unit,
) {
    val words = words()
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
            modifier =
                Modifier.weight(1f)
                    .fadedTowardTheEnd(state.canScrollForward)
                    .testTag("band-chips"),
        ) {
            itemsIndexed(curve.bands) { index, band ->
                val described =
                    stringResource(R.string.equaliser_band_named, index + 1, bandName(words, band))
                FilterChip(
                    selected = chosen == index,
                    onClick = { onSelect(index) },
                    label = { Text("${index + 1}") },
                    modifier =
                        Modifier.semantics { contentDescription = described }
                            .testTag("band-chip-$index"),
                )
            }
        }
        AssistChip(
            onClick = onAdd,
            enabled = curve.bands.size < bounds.bandsAtMost,
            label = { Text(stringResource(R.string.equaliser_add_band)) },
            leadingIcon = { Icon(Icons.Rounded.Add, null, Modifier.size(AssistChipDefaults.IconSize)) },
            colors =
                AssistChipDefaults.assistChipColors(
                    containerColor = MaterialTheme.colorScheme.secondaryContainer,
                    labelColor = MaterialTheme.colorScheme.onSecondaryContainer,
                    leadingIconContentColor = MaterialTheme.colorScheme.onSecondaryContainer,
                ),
            border = null,
            modifier = Modifier.padding(start = 8.dp).testTag("add-band"),
        )
    }
}

private val CHIPS_FADE_OVER = 48.dp

private fun Modifier.fadedTowardTheEnd(more: Boolean): Modifier =
    if (!more) {
        this
    } else {
        graphicsLayer { compositingStrategy = CompositingStrategy.Offscreen }
            .drawWithContent {
                drawContent()
                val fade = CHIPS_FADE_OVER.toPx()
                val (from, to) =
                    if (layoutDirection == LayoutDirection.Ltr) {
                        size.width - fade to size.width
                    } else {
                        fade to 0f
                    }
                drawRect(
                    Brush.horizontalGradient(
                        0f to Color.Black,
                        1f to Color.Transparent,
                        startX = from,
                        endX = to,
                    ),
                    blendMode = BlendMode.DstIn,
                )
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
    val words = words()
    val removeBand = stringResource(R.string.equaliser_remove_band, number)
    EqualiserCard(modifier.testTag("band-card")) {
        Column(Modifier.padding(start = 16.dp, end = 8.dp, top = 4.dp, bottom = 12.dp)) {
            FlowRow(
                Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                itemVerticalAlignment = Alignment.CenterVertically,
            ) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(
                        stringResource(R.string.equaliser_band, number),
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
                        Modifier.semantics { contentDescription = removeBand }
                            .testTag("remove-band"),
                ) {
                    Icon(Icons.Rounded.DeleteOutline, null, Modifier.size(ButtonDefaults.IconSize))
                    Spacer(Modifier.width(ButtonDefaults.IconSpacing))
                    Text(stringResource(R.string.equaliser_remove))
                }
            }
            Column(Modifier.padding(end = 8.dp)) {
                MeasureControl(
                    measure = gainMeasure(words, bounds),
                    value = band.gainDb,
                    description = stringResource(R.string.equaliser_band_gain, number),
                    onCommit = { onChange(band.copy(gainDb = tidyDb(it.coerceIn(bounds.gain)))) },
                    tag = "band-gain",
                    enabled = band.filter.usesGain,
                    unused =
                        stringResource(
                            R.string.equaliser_not_used_by,
                            stringResource(band.filter.label).lowercase(),
                        ),
                )
                MeasureControl(
                    measure = hertzMeasure(words, bounds),
                    value = band.frequencyHz,
                    description = stringResource(R.string.equaliser_band_frequency, number),
                    onCommit = {
                        onChange(band.copy(frequencyHz = tidyHertz(it.coerceIn(bounds.hertz))))
                    },
                    tag = "band-frequency",
                )
                MeasureControl(
                    measure = widthMeasure(words, bounds),
                    value = band.q,
                    description = stringResource(R.string.equaliser_band_width, number),
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
    val label = stringResource(filter.label)
    val described = stringResource(R.string.equaliser_band_type, number, label)
    Box {
        TextButton(
            onClick = { open = true },
            contentPadding = PaddingValues(start = 8.dp, end = 4.dp),
            modifier =
                Modifier.semantics { contentDescription = described }.testTag("band-type"),
        ) {
            Text(label, style = MaterialTheme.typography.titleMedium)
            Icon(Icons.Rounded.ArrowDropDown, null)
        }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            EqualiserFilter.entries.forEach { choice ->
                DropdownMenuItem(
                    text = { Text(stringResource(choice.label)) },
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
