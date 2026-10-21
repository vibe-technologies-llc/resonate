package org.resonate.android.ui.equaliser

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.DeleteOutline
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExposedDropdownMenu
import androidx.compose.material3.ExposedDropdownMenuAnchorType
import androidx.compose.material3.ExposedDropdownMenuBox
import androidx.compose.material3.ExposedDropdownMenuDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import org.resonate.android.nativecore.EqualiserBand
import org.resonate.android.nativecore.EqualiserFilter

@Composable
fun BandEditor(
    band: EqualiserBand,
    index: Int,
    bounds: EqualiserBounds,
    onChange: (EqualiserBand) -> Unit,
    onDelete: () -> Unit,
    modifier: Modifier = Modifier,
) {
    var liveHertz by remember(band) { mutableStateOf<Float?>(null) }
    var liveGain by remember(band) { mutableStateOf<Float?>(null) }
    var liveQ by remember(band) { mutableStateOf<Float?>(null) }
    val number = index + 1
    Card(
        modifier = modifier.fillMaxWidth(),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerHigh),
        shape = MaterialTheme.shapes.extraLarge,
    ) {
        Column(Modifier.padding(start = 20.dp, end = 12.dp, top = 12.dp, bottom = 16.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(
                    "Band $number",
                    style = MaterialTheme.typography.titleMedium,
                    modifier = Modifier.weight(1f).semantics { heading() },
                )
                TextButton(
                    onClick = onDelete,
                    colors = ButtonDefaults.textButtonColors(contentColor = MaterialTheme.colorScheme.error),
                ) {
                    Icon(Icons.Rounded.DeleteOutline, null, Modifier.padding(end = 8.dp))
                    Text("Delete")
                }
            }
            Spacer(Modifier.height(8.dp))
            FilterMenu(band.filter, Modifier.padding(end = 8.dp)) { onChange(band.copy(filter = it)) }
            Spacer(Modifier.height(16.dp))
            Measure("Frequency", hertzLabel(liveHertz ?: band.frequencyHz))
            LogCommitSlider(
                value = band.frequencyHz.coerceIn(bounds.hertz),
                range = bounds.hertz,
                description = "Band $number frequency",
                format = ::hertzLabel,
                onDraft = { liveHertz = it },
                onCommit = { onChange(band.copy(frequencyHz = tidyHertz(it.coerceIn(bounds.hertz)))) },
            )
            if (band.filter.usesGain) {
                Measure("Gain", signedDb(liveGain ?: band.gainDb))
                CommitSlider(
                    value = band.gainDb.coerceIn(bounds.gain),
                    range = bounds.gain,
                    description = "Band $number gain",
                    stateText = ::signedDb,
                    snap = ::tidyDb,
                    look = SliderLook.CENTRED,
                    onDraft = { liveGain = it },
                    onCommit = { onChange(band.copy(gainDb = tidyDb(it.coerceIn(bounds.gain)))) },
                )
            }
            Measure("Width", qLabel(liveQ ?: band.q))
            LogCommitSlider(
                value = band.q.coerceIn(bounds.q),
                range = bounds.q,
                description = "Band $number Q",
                format = ::qLabel,
                onDraft = { liveQ = it },
                onCommit = { onChange(band.copy(q = tidyQ(it.coerceIn(bounds.q)))) },
            )
        }
    }
}

@Composable
private fun Measure(label: String, value: String) {
    Row(Modifier.fillMaxWidth().padding(top = 8.dp, end = 8.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(
            label,
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.weight(1f),
        )
        Text(
            value,
            style = MaterialTheme.typography.titleSmall.copy(fontFeatureSettings = "tnum"),
            color = MaterialTheme.colorScheme.primary,
        )
    }
}

@Composable
private fun FilterMenu(filter: EqualiserFilter, modifier: Modifier = Modifier, onChoose: (EqualiserFilter) -> Unit) {
    var open by remember { mutableStateOf(false) }
    ExposedDropdownMenuBox(expanded = open, onExpandedChange = { open = it }, modifier = modifier) {
        OutlinedTextField(
            value = filter.label,
            onValueChange = {},
            readOnly = true,
            singleLine = true,
            label = { Text("Type") },
            trailingIcon = { ExposedDropdownMenuDefaults.TrailingIcon(expanded = open) },
            modifier = Modifier.fillMaxWidth().menuAnchor(ExposedDropdownMenuAnchorType.PrimaryNotEditable),
        )
        ExposedDropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            EqualiserFilter.entries.forEach { choice ->
                DropdownMenuItem(
                    text = { Text(choice.label) },
                    onClick = {
                        open = false
                        if (choice != filter) onChoose(choice)
                    },
                    contentPadding = ExposedDropdownMenuDefaults.ItemContentPadding,
                )
            }
        }
    }
}
