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
import androidx.compose.material.icons.rounded.Healing
import androidx.compose.material.icons.rounded.GraphicEq
import androidx.compose.material.icons.rounded.Headphones
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
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.resonate.android.nativecore.CrossfeedLevel
import org.resonate.android.nativecore.GainMode
import org.resonate.android.nativecore.LossyRestoration
import org.resonate.android.nativecore.PlaybackPreferences
import org.resonate.android.nativecore.ResamplingPhase
import org.resonate.android.nativecore.ResamplingQuality
import org.resonate.android.R
import org.resonate.android.ui.Resonate
import org.resonate.android.ui.components.Page
import org.resonate.android.ui.components.SectionHeader
import org.resonate.android.ui.components.CommitSlider
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

    fun crossfeed(level: CrossfeedLevel) = change { it.copy(crossfeed = level) }

    fun truePeak(enabled: Boolean) = change { it.copy(truePeak = enabled) }

    fun dsdLikePcm(enabled: Boolean) = change { it.copy(dsdLikePcm = enabled) }

    fun buffer(ms: Int) = change { it.copy(bufferMs = ms.toUInt()) }
}

@Composable
fun ProcessingScreen() {
    val model = resonateViewModel { ProcessingViewModel(it) }
    val preferences by model.preferences.collectAsStateWithLifecycle()
    Page(stringResource(R.string.settings_processing_title)) { padding ->
        val current = preferences
        if (current == null) {
            Loading(padding)
            return@Page
        }
        SettingsList(padding, Modifier.testTag("processing-list")) {
            item(key = "volume") { SectionHeader(stringResource(R.string.processing_volume)) }
            item(key = "replay-gain") { ReplayGain(current.replayGain, model::replayGain) }
            item(key = "true-peak") {
                SwitchRow(
                    title = stringResource(R.string.processing_true_peak),
                    icon = Icons.AutoMirrored.Rounded.ShowChart,
                    supporting = stringResource(R.string.processing_true_peak_detail),
                    checked = current.truePeak,
                    onCheckedChange = model::truePeak,
                    modifier = Modifier.testTag("true-peak"),
                )
            }
            item(key = "resampling") {
                SectionHeader(stringResource(R.string.processing_resampling))
            }
            item(key = "quality") {
                ChoiceRow(
                    title = stringResource(R.string.settings_inner_resampling_quality),
                    icon = Icons.Rounded.GraphicEq,
                    choices = qualityChoices,
                    selected = current.quality,
                    onChoose = model::quality,
                    modifier = Modifier.testTag("quality"),
                )
            }
            item(key = "phase") {
                ChoiceRow(
                    title = stringResource(R.string.settings_inner_filter_phase),
                    icon = Icons.Rounded.Waves,
                    choices = phaseChoices,
                    selected = current.phase,
                    onChoose = model::phase,
                    modifier = Modifier.testTag("phase"),
                )
            }
            item(key = "stereo") { SectionHeader(stringResource(R.string.processing_stereo)) }
            item(key = "crossfeed") {
                ChoiceRow(
                    title = stringResource(R.string.processing_crossfeed),
                    icon = Icons.Rounded.Headphones,
                    choices = crossfeedChoices,
                    selected = current.crossfeed,
                    onChoose = model::crossfeed,
                    modifier = Modifier.testTag("crossfeed"),
                )
            }
            item(key = "crossfeed-note") {
                Footnote(stringResource(R.string.processing_crossfeed_note))
            }
            item(key = "sources") { SectionHeader(stringResource(R.string.processing_sources)) }
            item(key = "dsd") {
                SwitchRow(
                    title = stringResource(R.string.settings_inner_play_dsd_at_pcm_level),
                    icon = Icons.Rounded.Memory,
                    supporting = stringResource(R.string.processing_dsd_detail),
                    checked = current.dsdLikePcm,
                    onCheckedChange = model::dsdLikePcm,
                )
            }
            item(key = "restoration") { Restoration(current.restoration, model::restoration) }
            item(key = "buffer-header") {
                SectionHeader(stringResource(R.string.settings_inner_playback_buffer))
            }
            item(key = "buffer") { Buffer(current.bufferMs.toInt(), model::buffer) }
            item(key = "note") {
                Footnote(stringResource(R.string.processing_note))
            }
        }
    }
}

@Composable
private fun ReplayGain(selected: GainMode, onChoose: (GainMode) -> Unit) {
    val choices = gainChoices
    Column(Modifier.fillMaxWidth()) {
        SettingRow(
            title = "ReplayGain",
            icon = Icons.AutoMirrored.Rounded.VolumeUp,
            supporting = choices.first { it.value == selected }.detail,
        )
        SingleChoiceSegmentedButtonRow(
            Modifier.fillMaxWidth()
                .padding(start = 56.dp, end = Sizes.gutter, bottom = 8.dp)
                .testTag("replay-gain"),
        ) {
            choices.forEachIndexed { index, choice ->
                SegmentedButton(
                    selected = choice.value == selected,
                    onClick = { onChoose(choice.value) },
                    shape = SegmentedButtonDefaults.itemShape(index, choices.size),
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
        leadingContent = { SettingIcon(Icons.Rounded.Healing) },
        supportingContent = {
            Column {
                Text(
                    stringResource(
                        R.string.processing_restoration_line,
                        labelOf(restorationChoices, selected),
                    )
                )
                Spacer(Modifier.height(6.dp))
                ExperimentalBadge()
            }
        },
        colors = ListItemDefaults.colors(containerColor = Color.Transparent),
    ) {
        Text(stringResource(R.string.settings_inner_lossy_restoration))
    }
    if (open) {
        ChoiceDialog(
            stringResource(R.string.settings_inner_lossy_restoration),
            restorationChoices,
            selected,
            onChoose,
        ) {
            open = false
        }
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
            Text(
                stringResource(R.string.processing_experimental),
                style = MaterialTheme.typography.labelMedium,
            )
        }
    }
}

@Composable
private fun Buffer(stored: Int, onCommit: (Int) -> Unit) {
    var draft by remember { mutableStateOf<Float?>(null) }
    val shown = draft?.let { bufferStep(it) } ?: stored
    val length = stringResource(R.string.processing_buffer_length)
    val spoken = pluralStringResource(R.plurals.processing_buffer_spoken, shown, shown)
    Column(Modifier.fillMaxWidth()) {
        SettingRow(
            title = length,
            icon = Icons.Rounded.Timer,
            supporting = stringResource(R.string.processing_buffer_detail),
            trailing = {
                Text(
                    stringResource(R.string.processing_buffer_ms, shown),
                    style = MaterialTheme.typography.labelLarge,
                    color = MaterialTheme.colorScheme.primary,
                )
            },
        )
        CommitSlider(
            value = stored.toFloat(),
            onCommit = { onCommit(bufferStep(it)) },
            onDraft = { draft = it },
            range = BUFFER_LEAST_MS.toFloat()..BUFFER_MOST_MS.toFloat(),
            trackHeight = 4.dp,
            stateText = { spoken },
            modifier =
                Modifier.fillMaxWidth()
                    .padding(start = 56.dp, end = Sizes.gutter)
                    .semantics { contentDescription = length }
                    .testTag("buffer"),
        )
    }
}
