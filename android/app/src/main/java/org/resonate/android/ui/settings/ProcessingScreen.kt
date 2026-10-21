package org.resonate.android.ui.settings

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.ShowChart
import androidx.compose.material.icons.automirrored.rounded.VolumeUp
import androidx.compose.material.icons.rounded.AutoFixHigh
import androidx.compose.material.icons.rounded.GraphicEq
import androidx.compose.material.icons.rounded.Memory
import androidx.compose.material.icons.rounded.Science
import androidx.compose.material.icons.rounded.Timer
import androidx.compose.material.icons.rounded.Waves
import androidx.compose.material3.Icon
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Slider
import androidx.compose.material3.SliderState
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.getValue
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.resonate.android.nativecore.GainMode
import org.resonate.android.nativecore.LossyRestoration
import org.resonate.android.nativecore.PlaybackPreferences
import org.resonate.android.nativecore.ResamplingPhase
import org.resonate.android.nativecore.ResamplingQuality
import org.resonate.android.ui.Resonate
import org.resonate.android.ui.components.Page
import org.resonate.android.ui.components.SectionHeader
import org.resonate.android.ui.components.Sizes
import org.resonate.android.ui.resonateViewModel

class ProcessingViewModel(resonate: Resonate) : ViewModel() {
    private val library = resonate.library

    val preferences = library.preferences

    private fun change(edit: (PlaybackPreferences) -> PlaybackPreferences) =
        library.updatePreferences(edit)

    fun replayGain(mode: GainMode) = change { it.copy(replayGain = mode) }

    fun quality(value: ResamplingQuality) = change { it.copy(quality = value) }

    fun phase(value: ResamplingPhase) = change { it.copy(phase = value) }

    fun restoration(value: LossyRestoration) = change { it.copy(restoration = value) }

    fun truePeak(enabled: Boolean) = change { it.copy(truePeak = enabled) }

    fun dsdLikePcm(enabled: Boolean) = change { it.copy(dsdLikePcm = enabled) }

    fun buffer(ms: Int) = change { it.copy(bufferMs = ms.toUInt()) }
}

@Composable
fun ProcessingScreen() {
    val model = resonateViewModel { ProcessingViewModel(it) }
    val preferences by model.preferences.collectAsStateWithLifecycle()
    Page("Sound processing") { padding ->
        val current = preferences
        if (current == null) {
            Loading(padding)
            return@Page
        }
        SettingsList(padding, Modifier.testTag("processing-list")) {
            item(key = "volume") { SectionHeader("Volume") }
            item(key = "replay-gain") { ReplayGain(current.replayGain, model::replayGain) }
            item(key = "true-peak") {
                SwitchRow(
                    title = "Prevent clipping",
                    icon = Icons.AutoMirrored.Rounded.ShowChart,
                    supporting =
                        "Watches for true peaks between samples and lowers the level only where one would clip",
                    checked = current.truePeak,
                    onCheckedChange = model::truePeak,
                    modifier = Modifier.testTag("true-peak"),
                )
            }
            item(key = "resampling") { SectionHeader("Resampling") }
            item(key = "quality") {
                ChoiceRow(
                    title = "Resampling quality",
                    icon = Icons.Rounded.GraphicEq,
                    choices = qualityChoices,
                    selected = current.quality,
                    onChoose = model::quality,
                    modifier = Modifier.testTag("quality"),
                )
            }
            item(key = "phase") {
                ChoiceRow(
                    title = "Filter phase",
                    icon = Icons.Rounded.Waves,
                    choices = phaseChoices,
                    selected = current.phase,
                    onChoose = model::phase,
                    modifier = Modifier.testTag("phase"),
                )
            }
            item(key = "sources") { SectionHeader("Sources") }
            item(key = "dsd") {
                SwitchRow(
                    title = "Play DSD at PCM level",
                    icon = Icons.Rounded.Memory,
                    supporting = "Raise converted DSD by about 6 dB to match PCM loudness",
                    checked = current.dsdLikePcm,
                    onCheckedChange = model::dsdLikePcm,
                )
            }
            item(key = "restoration") { Restoration(current.restoration, model::restoration) }
            item(key = "buffer-header") { SectionHeader("Playback buffer") }
            item(key = "buffer") { Buffer(current.bufferMs.toInt(), model::buffer) }
            item(key = "note") {
                Footnote(
                    "Processing runs inside Resonate's own engine, before the audio reaches Android."
                )
            }
        }
    }
}

@Composable
private fun ReplayGain(selected: GainMode, onChoose: (GainMode) -> Unit) {
    Column(Modifier.fillMaxWidth()) {
        SettingRow(
            title = "ReplayGain",
            icon = Icons.AutoMirrored.Rounded.VolumeUp,
            supporting = gainChoices.first { it.value == selected }.detail,
        )
        SingleChoiceSegmentedButtonRow(
            Modifier.fillMaxWidth()
                .padding(start = 56.dp, end = Sizes.gutter, bottom = 8.dp)
                .testTag("replay-gain"),
        ) {
            gainChoices.forEachIndexed { index, choice ->
                SegmentedButton(
                    selected = choice.value == selected,
                    onClick = { onChoose(choice.value) },
                    shape = SegmentedButtonDefaults.itemShape(index, gainChoices.size),
                ) {
                    Text(choice.label, maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
            }
        }
    }
}

@Composable
private fun Restoration(selected: LossyRestoration, onChoose: (LossyRestoration) -> Unit) {
    var open by remember { mutableStateOf(false) }
    ListItem(
        onClick = { open = true },
        modifier = Modifier.testTag("restoration"),
        leadingContent = { SettingIcon(Icons.Rounded.AutoFixHigh) },
        supportingContent = {
            Column {
                Text("${labelOf(restorationChoices, selected)} · MP3, AAC and Vorbis only")
                Spacer(Modifier.height(6.dp))
                ExperimentalBadge()
            }
        },
        colors = ListItemDefaults.colors(containerColor = Color.Transparent),
    ) {
        Text("Lossy restoration")
    }
    if (open) {
        ChoiceDialog("Lossy restoration", restorationChoices, selected, onChoose) { open = false }
    }
}

@Composable
private fun ExperimentalBadge() {
    Surface(
        color = MaterialTheme.colorScheme.tertiaryContainer,
        contentColor = MaterialTheme.colorScheme.onTertiaryContainer,
        shape = MaterialTheme.shapes.small,
    ) {
        Row(
            Modifier.padding(horizontal = 8.dp, vertical = 4.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(Icons.Rounded.Science, contentDescription = null, modifier = Modifier.padding(end = 4.dp).size(16.dp))
            Text("Experimental", style = MaterialTheme.typography.labelMedium)
        }
    }
}

@Composable
private fun Buffer(stored: Int, onCommit: (Int) -> Unit) {
    var dragging by remember { mutableStateOf(false) }
    val slider = remember {
        SliderState(
            value = stored.toFloat(),
            trackRange = BUFFER_LEAST_MS.toFloat()..BUFFER_MOST_MS.toFloat(),
        )
    }
    LaunchedEffect(stored) { if (!dragging) slider.value = stored.toFloat() }
    val shown = if (dragging) bufferStep(slider.value) else stored
    Column(Modifier.fillMaxWidth()) {
        SettingRow(
            title = "Buffer length",
            icon = Icons.Rounded.Timer,
            supporting = "Longer rides out a busy phone; shorter answers controls sooner",
            trailing = {
                Text(
                    "$shown ms",
                    style = MaterialTheme.typography.labelLarge,
                    color = MaterialTheme.colorScheme.primary,
                )
            },
        )
        Slider(
            state = slider,
            onValueChange = {
                dragging = true
                slider.value = it
            },
            onValueChangeFinished = {
                onCommit(bufferStep(slider.value))
                dragging = false
            },
            modifier =
                Modifier.fillMaxWidth()
                    .padding(start = 56.dp, end = Sizes.gutter)
                    .semantics {
                        contentDescription = "Buffer length"
                        stateDescription = "$shown milliseconds"
                    }
                    .testTag("buffer"),
        )
    }
}
