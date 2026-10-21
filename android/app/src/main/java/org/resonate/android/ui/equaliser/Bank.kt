package org.resonate.android.ui.equaliser

import androidx.compose.animation.animateColorAsState
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import org.resonate.android.nativecore.EqualiserBand
import org.resonate.android.nativecore.EqualiserCurve
import org.resonate.android.nativecore.EqualiserFilter

private data class Touched(val index: Int, val gainDb: Float)

@Composable
fun SliderBank(
    curve: EqualiserCurve,
    bounds: EqualiserBounds,
    onCommit: (index: Int, gainDb: Float) -> Unit,
    onEdit: (index: Int) -> Unit,
    modifier: Modifier = Modifier,
) {
    val gained = curve.bands.indices.filter { curve.bands[it].filter.usesGain }.sortedBy { curve.bands[it].frequencyHz }
    val gainless = curve.bands.indices.filterNot { curve.bands[it].filter.usesGain }
    val cap = bankCap(curve, bounds)
    val style = MaterialTheme.typography.labelMedium
    val measurer = rememberTextMeasurer()
    val density = LocalDensity.current
    val labelDp =
        remember(gained, style, density) {
            val samples = listOf("+${cap.toInt()}.0") + gained.map { shortHertz(curve.bands[it].frequencyHz) }
            samples.maxOf { measurer.measure(it, style).size.width }.let { with(density) { it.toDp().value } }
        }
    BoxWithConstraints(modifier.fillMaxWidth()) {
        Column {
            if (gained.isNotEmpty()) {
                if (bankFits(gained.size, this@BoxWithConstraints.maxWidth.value, labelDp)) {
                    VerticalBank(curve, gained, cap, onCommit)
                } else {
                    gained.forEachIndexed { position, index ->
                        if (position > 0) HorizontalDivider(Modifier.padding(horizontal = 16.dp))
                        BandRow(curve.bands[index], cap) { onCommit(index, it) }
                    }
                }
            }
            gainless.forEach { index ->
                val band = curve.bands[index]
                ListItem(
                    colors = ListItemDefaults.colors(containerColor = Color.Transparent),
                    supportingContent = { Text(band.filter.label) },
                    trailingContent = { TextButton(onClick = { onEdit(index) }) { Text("Edit") } },
                ) { Text(hertzLabel(band.frequencyHz)) }
            }
        }
    }
}

@Composable
private fun VerticalBank(
    curve: EqualiserCurve,
    gained: List<Int>,
    cap: Float,
    onCommit: (index: Int, gainDb: Float) -> Unit,
) {
    var touched by remember { mutableStateOf<Touched?>(null) }
    Column(Modifier.padding(horizontal = 8.dp, vertical = 12.dp)) {
        BankHeadline(touched?.let { curve.bands.getOrNull(it.index)?.copy(gainDb = it.gainDb) })
        Spacer(Modifier.height(12.dp))
        Row(Modifier.fillMaxWidth().testTag("equaliser-bank")) {
            gained.forEach { index ->
                val band = curve.bands[index]
                val live = touched?.takeIf { it.index == index }?.gainDb
                val gain = live ?: band.gainDb
                val tint by
                    animateColorAsState(
                        when {
                            live != null -> MaterialTheme.colorScheme.primary
                            gain != 0f -> MaterialTheme.colorScheme.onSurface
                            else -> MaterialTheme.colorScheme.onSurfaceVariant
                        },
                        label = "band value",
                    )
                Column(Modifier.weight(1f), horizontalAlignment = Alignment.CenterHorizontally) {
                    Text(
                        signedDb(gain).removeSuffix(" dB"),
                        style = MaterialTheme.typography.labelMedium.copy(fontFeatureSettings = "tnum"),
                        color = tint,
                        maxLines = 1,
                        textAlign = TextAlign.Center,
                    )
                    Spacer(Modifier.height(8.dp))
                    Box(Modifier.height(BANK_HEIGHT), contentAlignment = Alignment.Center) {
                        CommitSlider(
                            value = band.gainDb,
                            range = -cap..cap,
                            description = bandName(band),
                            stateText = ::signedDb,
                            onCommit = { onCommit(index, it) },
                            onDraft = { moving -> touched = moving?.let { Touched(index, it) } },
                            snap = ::quarterDb,
                            vertical = true,
                            look = SliderLook.CENTRED,
                            modifier = Modifier.height(BANK_HEIGHT).testTag("band-$index"),
                        )
                    }
                    Spacer(Modifier.height(8.dp))
                    Text(
                        shortHertz(band.frequencyHz),
                        style = MaterialTheme.typography.labelMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 1,
                        textAlign = TextAlign.Center,
                    )
                }
            }
        }
    }
}

@Composable
private fun BankHeadline(band: EqualiserBand?) {
    Row(
        Modifier.fillMaxWidth().padding(horizontal = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (band == null) {
            Text(
                "Slide a band up to boost it, down to cut it",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.weight(1f),
            )
        } else {
            Text(
                "${audibleRange(band.frequencyHz)} · ${hertzLabel(band.frequencyHz)}",
                style = MaterialTheme.typography.titleSmall,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f),
            )
            Text(
                signedDb(band.gainDb),
                style = MaterialTheme.typography.titleSmall.copy(fontFeatureSettings = "tnum"),
                fontWeight = FontWeight.SemiBold,
                color = MaterialTheme.colorScheme.primary,
            )
        }
    }
}

@Composable
private fun BandRow(band: EqualiserBand, cap: Float, onCommit: (Float) -> Unit) {
    var live by remember { mutableStateOf<Float?>(null) }
    val gain = live ?: band.gainDb
    Column(Modifier.padding(horizontal = 16.dp, vertical = 12.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text(audibleRange(band.frequencyHz), style = MaterialTheme.typography.titleSmall)
                Text(
                    hertzLabel(band.frequencyHz) +
                        if (band.filter == EqualiserFilter.PEAKING) ""
                        else " · ${band.filter.label}",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            Text(
                signedDb(gain),
                style = MaterialTheme.typography.titleSmall.copy(fontFeatureSettings = "tnum"),
                color = if (live != null || gain != 0f) MaterialTheme.colorScheme.primary
                else MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        CommitSlider(
            value = band.gainDb,
            range = -cap..cap,
            description = bandName(band),
            stateText = ::signedDb,
            onCommit = onCommit,
            onDraft = { live = it },
            snap = ::quarterDb,
            look = SliderLook.CENTRED,
        )
    }
}

private val BANK_HEIGHT = 220.dp
