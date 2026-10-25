package org.resonate.android.ui.settings

import android.media.AudioDeviceCallback
import android.media.AudioDeviceInfo
import android.media.AudioManager
import android.os.Handler
import android.os.Looper
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.BluetoothAudio
import androidx.compose.material.icons.rounded.Headphones
import androidx.compose.material.icons.rounded.Hearing
import androidx.compose.material.icons.rounded.Refresh
import androidx.compose.material.icons.rounded.Speaker
import androidx.compose.material.icons.rounded.SpeakerGroup
import androidx.compose.material.icons.rounded.SwapHoriz
import androidx.compose.material.icons.rounded.Tv
import androidx.compose.material.icons.rounded.Usb
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.DisposableEffect
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn
import org.resonate.android.AudioOutput
import org.resonate.android.ui.Resonate
import org.resonate.android.ui.components.Page
import org.resonate.android.ui.components.SectionHeader
import org.resonate.android.ui.components.Sizes
import org.resonate.android.ui.resonateViewModel

class OutputViewModel(resonate: Resonate) : ViewModel() {
    private val features = resonate.features
    private val library = resonate.library

    val outputs = features.outputs
    val selected = features.selectedOutput
    val actual = features.actualOutput

    val clamped: StateFlow<ULong> =
        library.snapshot
            .map { it?.overs ?: 0uL }
            .distinctUntilChanged()
            .stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), 0uL)

    fun refresh() = features.refreshOutputs()

    fun select(id: Int) = features.selectOutput(id)
}

private fun iconFor(output: AudioOutput): ImageVector =
    when (output.kind) {
        AUTOMATIC_KIND -> Icons.Rounded.SpeakerGroup
        "Earpiece" -> Icons.Rounded.Hearing
        "Wired headphones",
        "Wired headset" -> Icons.Rounded.Headphones
        "Bluetooth audio" -> Icons.Rounded.BluetoothAudio
        "USB audio" -> Icons.Rounded.Usb
        "HDMI audio" -> Icons.Rounded.Tv
        else -> Icons.Rounded.Speaker
    }

@Composable
private fun FollowDevices(refresh: () -> Unit) {
    val context = LocalContext.current
    DisposableEffect(context) {
        val manager = context.getSystemService(AudioManager::class.java)
        val callback =
            object : AudioDeviceCallback() {
                override fun onAudioDevicesAdded(addedDevices: Array<out AudioDeviceInfo>) =
                    refresh()

                override fun onAudioDevicesRemoved(removedDevices: Array<out AudioDeviceInfo>) =
                    refresh()
            }
        manager?.registerAudioDeviceCallback(callback, Handler(Looper.getMainLooper()))
        refresh()
        onDispose { manager?.unregisterAudioDeviceCallback(callback) }
    }
}

@Composable
fun OutputScreen() {
    val model = resonateViewModel { OutputViewModel(it) }
    val outputs by model.outputs.collectAsStateWithLifecycle()
    val selected by model.selected.collectAsStateWithLifecycle()
    val actual by model.actual.collectAsStateWithLifecycle()
    val clamped by model.clamped.collectAsStateWithLifecycle()
    FollowDevices(model::refresh)
    val choices = withSystemDefault(outputs)
    Page(
        "Audio output",
        actions = {
            IconButton(onClick = model::refresh) {
                Icon(Icons.Rounded.Refresh, contentDescription = "Look for outputs again")
            }
        },
    ) { padding ->
        SettingsList(padding, Modifier.testTag("output-list")) {
            item(key = "status") {
                RouteCard(
                    playing = choices.firstOrNull { it.id == actual },
                    playingName = outputNamed(outputs, actual),
                    pending = if (selected != actual) outputNamed(outputs, selected) else null,
                )
            }
            clampedSamplesWords(clamped)?.let { words ->
                item(key = "clamped") {
                    SettingRow(
                        title = "Clamped samples",
                        supporting = CLAMPED_SAMPLES_NOTE,
                        trailing = { Text(words) },
                        modifier = Modifier.testTag("clamped-samples"),
                    )
                }
            }
            item(key = "choose") { SectionHeader("Play through") }
            item(key = "choices") {
                Column(Modifier.selectableGroup()) {
                    choices.forEach { output ->
                        RadioRow(
                            title = outputTitle(output),
                            supporting =
                                listOfNotNull(outputSubtitle(output), outputRates(output))
                                    .joinToString(" · "),
                            icon = iconFor(output),
                            selected = output.id == selected,
                            onClick = { model.select(output.id) },
                            modifier = Modifier.testTag("output-${output.id}"),
                        )
                    }
                }
            }
            item(key = "note") {
                Footnote(
                    "Android owns the physical route: choosing an output asks for it, and Android may still send sound elsewhere. Resonate plays shared stereo audio, so this is no promise of a DAC's own rate or of bit-perfect delivery."
                )
            }
        }
    }
}

@Composable
private fun RouteCard(playing: AudioOutput?, playingName: String, pending: String?) {
    val scheme = MaterialTheme.colorScheme
    Card(
        modifier =
            Modifier.fillMaxWidth()
                .padding(horizontal = Sizes.gutter, vertical = 8.dp)
                .semantics { liveRegion = LiveRegionMode.Polite }
                .testTag("route"),
        shape = MaterialTheme.shapes.extraLarge,
        colors =
            CardDefaults.cardColors(
                containerColor = scheme.primaryContainer,
                contentColor = scheme.onPrimaryContainer,
            ),
    ) {
        Row(Modifier.padding(20.dp), verticalAlignment = Alignment.CenterVertically) {
            Surface(
                color = scheme.primary,
                contentColor = scheme.onPrimary,
                shape = MaterialTheme.shapes.large,
                modifier = Modifier.size(56.dp),
            ) {
                Icon(
                    playing?.let(::iconFor) ?: Icons.Rounded.Speaker,
                    contentDescription = null,
                    modifier = Modifier.padding(16.dp),
                )
            }
            Spacer(Modifier.width(16.dp))
            Column(Modifier.weight(1f)) {
                Text("Playing through", style = MaterialTheme.typography.labelLarge)
                Text(playingName, style = MaterialTheme.typography.titleLarge)
                playing?.let(::outputSubtitle)?.takeIf { it != playingName }?.let {
                    Text(it, style = MaterialTheme.typography.bodyMedium)
                }
            }
        }
        if (pending != null) {
            Row(
                Modifier.padding(start = 20.dp, end = 20.dp, bottom = 16.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Icon(
                    Icons.Rounded.SwapHoriz,
                    contentDescription = null,
                    modifier = Modifier.size(20.dp),
                )
                Spacer(Modifier.width(8.dp))
                Text(
                    "Asked for $pending; Android has not moved the sound there yet",
                    style = MaterialTheme.typography.bodyMedium,
                )
            }
        }
    }
}
