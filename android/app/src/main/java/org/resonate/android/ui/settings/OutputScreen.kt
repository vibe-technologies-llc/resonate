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
import androidx.compose.ui.res.stringResource
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
import org.resonate.android.R
import org.resonate.android.ui.Resonate
import org.resonate.android.ui.components.Page
import org.resonate.android.ui.components.SectionHeader
import org.resonate.android.ui.components.Sizes
import org.resonate.android.ui.resonateViewModel
import org.resonate.android.words

class OutputViewModel(resonate: Resonate) : ViewModel() {
    private val features = resonate.features
    private val library = resonate.library

    val outputs = features.outputs
    val selected = features.selectedOutput
    val actual = features.actualOutput
    val stream = features.outputStream
    val followsSongRate = library.followsSongRate

    val clamped: StateFlow<ULong> =
        library.snapshot
            .map { it?.overs ?: 0uL }
            .distinctUntilChanged()
            .stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), 0uL)

    fun refresh() = features.refreshOutputs()

    fun select(id: Int) = features.selectOutput(id)

    fun followSongRate(follows: Boolean) = library.followSongRate(follows)
}

private fun iconFor(output: AudioOutput): ImageVector =
    if (output.isSystemDefault) Icons.Rounded.SpeakerGroup
    else
        when (output.type) {
            AudioDeviceInfo.TYPE_BUILTIN_EARPIECE -> Icons.Rounded.Hearing
            AudioDeviceInfo.TYPE_WIRED_HEADPHONES,
            AudioDeviceInfo.TYPE_WIRED_HEADSET -> Icons.Rounded.Headphones
            AudioDeviceInfo.TYPE_BLUETOOTH_A2DP,
            AudioDeviceInfo.TYPE_BLUETOOTH_SCO,
            AudioDeviceInfo.TYPE_BLE_HEADSET,
            AudioDeviceInfo.TYPE_BLE_SPEAKER,
            AudioDeviceInfo.TYPE_BLE_BROADCAST -> Icons.Rounded.BluetoothAudio
            AudioDeviceInfo.TYPE_USB_DEVICE,
            AudioDeviceInfo.TYPE_USB_HEADSET,
            AudioDeviceInfo.TYPE_USB_ACCESSORY -> Icons.Rounded.Usb
            AudioDeviceInfo.TYPE_HDMI,
            AudioDeviceInfo.TYPE_HDMI_ARC,
            AudioDeviceInfo.TYPE_HDMI_EARC -> Icons.Rounded.Tv
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
    val stream by model.stream.collectAsStateWithLifecycle()
    val followsSongRate by model.followsSongRate.collectAsStateWithLifecycle()
    FollowDevices(model::refresh)
    val words = words()
    val choices = offeredOutputs(outputs)
    val playing = withSystemDefault(outputs).firstOrNull { it.id == actual }
    Page(
        stringResource(R.string.settings_output_title),
        actions = {
            IconButton(onClick = model::refresh) {
                Icon(
                    Icons.Rounded.Refresh,
                    contentDescription = stringResource(R.string.output_refresh),
                )
            }
        },
    ) { padding ->
        SettingsList(padding, Modifier.testTag("output-list")) {
            item(key = "status") {
                RouteCard(
                    playing = playing,
                    playingName = outputNamed(words, outputs, actual),
                    pending =
                        pendingOutput(selected, actual)?.let { outputNamed(words, outputs, it) },
                )
            }
            stream?.let { open ->
                item(key = "stream") {
                    SettingRow(
                        title = stringResource(R.string.output_current_stream),
                        supporting = outputPathWords(words, open.path),
                        trailing = { Text(outputStreamWords(words, open)) },
                        modifier = Modifier.testTag("output-stream"),
                    )
                }
            }
            playing?.let { outputTakes(words, it) }?.let { takes ->
                item(key = "takes") {
                    SettingRow(
                        title = stringResource(R.string.output_takes),
                        supporting = takes,
                        modifier = Modifier.testTag("output-takes"),
                    )
                }
            }
            item(key = "follow") {
                SwitchRow(
                    title = stringResource(R.string.output_follow_song_rate),
                    checked = followsSongRate,
                    onCheckedChange = model::followSongRate,
                    supporting = stringResource(R.string.output_follow_song_rate_note),
                    modifier = Modifier.testTag("follow-song-rate"),
                )
            }
            clampedSamplesWords(clamped)?.let { count ->
                item(key = "clamped") {
                    SettingRow(
                        title = stringResource(R.string.output_clamped),
                        supporting = stringResource(R.string.output_clamped_note),
                        trailing = { Text(count) },
                        modifier = Modifier.testTag("clamped-samples"),
                    )
                }
            }
            item(key = "choose") { SectionHeader(stringResource(R.string.output_play_through)) }
            item(key = "choices") {
                Column(Modifier.selectableGroup()) {
                    choices.forEach { output ->
                        RadioRow(
                            title = outputTitle(words, output),
                            supporting =
                                listOfNotNull(
                                        outputSubtitle(words, output),
                                        outputRates(words, output),
                                    )
                                    .joinToString(" · "),
                            icon = iconFor(output),
                            selected = standsFor(output, selected, outputs),
                            onClick = { model.select(output.id) },
                            modifier = Modifier.testTag("output-${output.id}"),
                        )
                    }
                }
            }
            item(key = "note") {
                Footnote(stringResource(R.string.output_note))
            }
        }
    }
}

@Composable
private fun RouteCard(playing: AudioOutput?, playingName: String, pending: String?) {
    val scheme = MaterialTheme.colorScheme
    val words = words()
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
                Text(
                    stringResource(R.string.output_playing_through),
                    style = MaterialTheme.typography.labelLarge,
                )
                Text(playingName, style = MaterialTheme.typography.titleLarge)
                playing?.let { outputSubtitle(words, it) }?.takeIf { it != playingName }?.let {
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
                    stringResource(R.string.output_pending, pending),
                    style = MaterialTheme.typography.bodyMedium,
                )
            }
        }
    }
}
