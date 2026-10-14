package org.resonate.android

import android.media.AudioDeviceCallback
import android.media.AudioDeviceInfo
import android.media.AudioManager
import android.os.Handler
import android.os.Looper
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.FilterChip
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Slider
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import org.resonate.android.nativecore.Snapshot

@Composable
fun OutputScreen(library: LibraryRepository, snapshot: Snapshot?) {
    val outputs by library.features.outputs.collectAsState()
    val selected by library.features.selectedOutput.collectAsState()
    val actual by library.features.actualOutput.collectAsState()
    val preferences by library.preferences.collectAsState()
    val audio = LocalContext.current.getSystemService(AudioManager::class.java)
    DisposableEffect(Unit) {
        val callback =
            object : AudioDeviceCallback() {
                override fun onAudioDevicesAdded(addedDevices: Array<out AudioDeviceInfo>) {
                    library.features.refreshOutputs()
                }

                override fun onAudioDevicesRemoved(removedDevices: Array<out AudioDeviceInfo>) {
                    library.features.refreshOutputs()
                }
            }
        audio.registerAudioDeviceCallback(callback, Handler(Looper.getMainLooper()))
        library.features.refreshOutputs()
        onDispose { audio.unregisterAudioDeviceCallback(callback) }
    }
    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(20.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        Text("Audio output", style = MaterialTheme.typography.headlineSmall)
        FilterChip(
            selected == 0,
            { library.features.selectOutput(0) },
            label = { Text("System default") },
        )
        outputs.forEach { output ->
            Column {
                FilterChip(
                    selected == output.id,
                    { library.features.selectOutput(output.id) },
                    label = { Text(output.name) },
                )
                if (actual == output.id && snapshot?.outputRate != 0u)
                    Text("Active route", color = MaterialTheme.colorScheme.primary)
                Text(
                    "Advertised rates: ${output.rates.takeIf { it.isNotEmpty() }?.joinToString { "$it Hz" } ?: "Chosen by Android"}",
                    style = MaterialTheme.typography.bodySmall,
                )
                Text(
                    "Advertised channels: ${output.channels.takeIf { it.isNotEmpty() }?.joinToString() ?: "Chosen by Android"}",
                    style = MaterialTheme.typography.bodySmall,
                )
            }
        }
        HorizontalDivider()
        Text("Current stream", style = MaterialTheme.typography.titleLarge)
        Text("Shared stereo float · ${snapshot?.outputRate?.takeIf { it > 0u } ?: 48000u} Hz")
        Text("Underruns: ${snapshot?.underruns ?: 0uL}")
        Text(
            "The system mixer and selected device determine the physical output format.",
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        preferences?.let { settings ->
            var buffer by
                remember(settings.bufferMs) { mutableFloatStateOf(settings.bufferMs.toFloat()) }
            Text(
                "Playback buffer: ${buffer.toInt()} ms",
                style = MaterialTheme.typography.titleMedium,
            )
            Slider(
                buffer,
                { buffer = it },
                valueRange = 50f..2000f,
                onValueChangeFinished = {
                    library.updatePreferences { it.copy(bufferMs = buffer.toUInt()) }
                },
            )
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                Text("Match DSD loudness to PCM", Modifier.weight(1f))
                Switch(
                    settings.dsdLikePcm,
                    { enabled -> library.updatePreferences { it.copy(dsdLikePcm = enabled) } },
                )
            }
        }
    }
}
