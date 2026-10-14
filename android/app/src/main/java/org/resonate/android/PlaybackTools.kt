package org.resonate.android

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Slider
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import java.util.Locale
import org.resonate.android.nativecore.EqualiserCurve
import org.resonate.android.nativecore.GainMode
import org.resonate.android.nativecore.LossyRestoration
import org.resonate.android.nativecore.ResamplingPhase
import org.resonate.android.nativecore.ResamplingQuality
import org.resonate.android.nativecore.SleepTimer
import org.resonate.android.nativecore.Snapshot
import org.resonate.android.nativecore.equaliserFrequencies
import org.resonate.android.nativecore.equaliserLimits
import org.resonate.android.nativecore.tenBandCurve
import org.resonate.android.nativecore.tenBandGains

@Composable
fun SoundSettings(library: LibraryRepository) {
    val preferences by library.preferences.collectAsState()
    val settings = preferences ?: return
    var profileName by rememberSaveable { mutableStateOf("") }
    var profileText by rememberSaveable { mutableStateOf("") }
    val limits = remember { equaliserLimits() }
    val gains = remember(settings.equaliser) { tenBandGains(settings.equaliser) }
    Column(
        Modifier.verticalScroll(rememberScrollState()).padding(20.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        Text("Equaliser", style = MaterialTheme.typography.titleLarge)
        ToggleSetting("Enable equaliser", settings.equaliserEnabled) { enabled ->
            library.updatePreferences { it.copy(equaliserEnabled = enabled) }
        }
        Text(
            "This curve applies to every Android audio route.",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        if (settings.equaliser.name.isNotEmpty())
            Text(
                "Active profile: ${settings.equaliser.name}",
                color = MaterialTheme.colorScheme.primary,
            )
        if (gains == null && settings.equaliser.bands.isNotEmpty())
            Text(
                "This curve has ${settings.equaliser.bands.size} parametric bands; " +
                    "moving a band slider replaces it with ten octave bands.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        LevelSetting(
            "Preamp",
            settings.equaliser.preampDb,
            -limits.preampAtMostDb..limits.preampAtMostDb,
        ) { gain ->
            library.updatePreferences { it.copy(equaliser = it.equaliser.copy(preampDb = gain)) }
        }
        val frequencies = remember { equaliserFrequencies() }
        frequencies.forEachIndexed { index, frequency ->
            val label =
                if (frequency < 1000) "${frequency.toString().removeSuffix(".0")} Hz"
                else "${frequency.toInt() / 1000} kHz"
            LevelSetting(label, gains?.getOrNull(index) ?: 0f, -12f..12f) { gain ->
                library.updatePreferences { current ->
                    val now = tenBandGains(current.equaliser) ?: frequencies.map { 0f }
                    current.copy(
                        equaliser =
                            tenBandCurve(
                                current.equaliser.preampDb,
                                now.mapIndexed { band, value ->
                                    if (band == index) gain else value
                                },
                            )
                    )
                }
            }
        }
        TextButton(
            onClick = {
                library.updatePreferences {
                    it.copy(equaliser = EqualiserCurve("", 0f, emptyList()))
                }
            }
        ) {
            Text("Reset curve")
        }
        var importing by rememberSaveable { mutableStateOf(false) }
        OutlinedButton(onClick = { importing = !importing }, modifier = Modifier.fillMaxWidth()) {
            Text(if (importing) "Close profile import" else "Import an equaliser profile")
        }
        if (importing) {
            androidx.compose.material3.OutlinedTextField(
                profileName,
                { profileName = it },
                label = { Text("Profile name") },
                modifier = Modifier.fillMaxWidth(),
            )
            androidx.compose.material3.OutlinedTextField(
                profileText,
                { profileText = it },
                label = { Text("EqualizerAPO / GraphicEQ profile") },
                modifier = Modifier.fillMaxWidth(),
                minLines = 3,
            )
            TextButton(
                onClick = {
                    library.features.applyProfileText(
                        profileName.ifBlank { "Imported profile" },
                        profileText,
                    )
                },
                enabled = profileText.isNotBlank(),
            ) {
                Text("Apply profile")
            }
        }
    }
}

@Composable
fun ProcessingSettings(library: LibraryRepository) {
    val snapshot by library.snapshot.collectAsState()
    val preferences by library.preferences.collectAsState()
    val settings = preferences ?: return
    Column(
        Modifier.verticalScroll(rememberScrollState()).padding(20.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        Text("Player volume", style = MaterialTheme.typography.titleMedium)
        var volume by remember(snapshot?.volume) { mutableFloatStateOf(snapshot?.volume ?: 1f) }
        Slider(
            volume,
            { volume = it },
            onValueChangeFinished = { library.request { it.setVolume(volume) } },
            modifier = Modifier.semantics { contentDescription = "Player volume" },
        )
        HorizontalDivider()
        ChoiceSetting(
            "ReplayGain",
            settings.replayGain,
            listOf(GainMode.OFF to "Off", GainMode.TRACK to "Track", GainMode.ALBUM to "Album"),
        ) { mode ->
            library.updatePreferences { it.copy(replayGain = mode) }
        }
        Text(
            "Uses the gain tags in your files to balance loudness.",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        ToggleSetting("True-peak clip prevention", settings.truePeak) { enabled ->
            library.updatePreferences { it.copy(truePeak = enabled) }
        }
        ChoiceSetting(
            "Resampling quality",
            settings.quality,
            listOf(
                ResamplingQuality.FAST to "Fast",
                ResamplingQuality.BALANCED to "Balanced",
                ResamplingQuality.HIGH to "High",
                ResamplingQuality.VERY_HIGH to "Very high",
            ),
        ) { quality ->
            library.updatePreferences { it.copy(quality = quality) }
        }
        ChoiceSetting(
            "Filter phase",
            settings.phase,
            listOf(
                ResamplingPhase.LINEAR to "Linear",
                ResamplingPhase.INTERMEDIATE to "Intermediate",
                ResamplingPhase.MINIMUM to "Minimum",
            ),
        ) { phase ->
            library.updatePreferences { it.copy(phase = phase) }
        }
        ChoiceSetting(
            "Lossy restoration",
            settings.restoration,
            listOf(
                LossyRestoration.OFF to "Off",
                LossyRestoration.REPAIR to "Repair",
                LossyRestoration.EXTEND to "Extend",
            ),
        ) { restoration ->
            library.updatePreferences { it.copy(restoration = restoration) }
        }
        Text(
            "Experimental processing for MP3, AAC and Vorbis. Lossless music passes through unchanged by restoration.",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

@Composable
private fun LevelSetting(
    label: String,
    value: Float,
    range: ClosedFloatingPointRange<Float>,
    commit: (Float) -> Unit,
) {
    var draft by remember(value) { mutableFloatStateOf(value) }
    Column {
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
            Text(label, style = MaterialTheme.typography.bodyMedium)
            Text(
                String.format(Locale.getDefault(), "%+.1f dB", draft),
                style = MaterialTheme.typography.labelMedium,
            )
        }
        Slider(
            value = draft,
            onValueChange = { draft = it },
            onValueChangeFinished = { commit(draft) },
            valueRange = range,
            modifier = Modifier.semantics { contentDescription = label },
        )
    }
}

@Composable
private fun ToggleSetting(label: String, value: Boolean, change: (Boolean) -> Unit) {
    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
        Text(label, Modifier.weight(1f), style = MaterialTheme.typography.bodyMedium)
        Switch(
            checked = value,
            onCheckedChange = change,
            modifier = Modifier.semantics { contentDescription = label },
        )
    }
}

@Composable
private fun <T> ChoiceSetting(
    label: String,
    value: T,
    choices: List<Pair<T, String>>,
    change: (T) -> Unit,
) {
    var expanded by remember { mutableStateOf(false) }
    Column {
        Text(label, style = MaterialTheme.typography.titleSmall)
        OutlinedButton(
            onClick = { expanded = true },
            modifier = Modifier.fillMaxWidth().semantics { contentDescription = label },
        ) {
            Text(choices.first { it.first == value }.second)
        }
        DropdownMenu(expanded = expanded, onDismissRequest = { expanded = false }) {
            choices.forEach { (choice, title) ->
                DropdownMenuItem(
                    text = { Text(title) },
                    onClick = {
                        expanded = false
                        change(choice)
                    },
                )
            }
        }
    }
}

@Composable
fun SleepTimerDialog(snapshot: Snapshot?, library: LibraryRepository, dismiss: () -> Unit) {
    AlertDialog(
        onDismissRequest = dismiss,
        title = { Text("Sleep timer") },
        text = {
            Column(Modifier.verticalScroll(rememberScrollState())) {
                Text(sleepDescription(snapshot), style = MaterialTheme.typography.bodyMedium)
                listOf(15u, 30u, 45u, 60u, 90u).forEach { minutes ->
                    TextButton(
                        onClick = {
                            library.request { it.setSleep(SleepTimer.After(minutes * 60u)) }
                            dismiss()
                        },
                        modifier = Modifier.fillMaxWidth(),
                    ) {
                        Text("$minutes minutes")
                    }
                }
                TextButton(
                    onClick = {
                        library.request { it.setSleep(SleepTimer.EndOfTrack) }
                        dismiss()
                    },
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Text("End of song")
                }
                TextButton(
                    onClick = {
                        library.request { it.setSleep(SleepTimer.EndOfQueue) }
                        dismiss()
                    },
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Text("End of queue")
                }
            }
        },
        confirmButton = { TextButton(onClick = dismiss) { Text("Done") } },
        dismissButton = {
            if (snapshot?.sleep != null)
                TextButton(
                    onClick = {
                        library.request { it.setSleep(null) }
                        dismiss()
                    }
                ) {
                    Text("Cancel timer")
                }
        },
    )
}

fun sleepDescription(snapshot: Snapshot?): String =
    when (snapshot?.sleep) {
        is SleepTimer.After -> {
            val seconds = ((snapshot.sleepRemainingMs ?: 0uL) + 999uL) / 1000uL
            "Pauses in ${seconds / 60uL}:${(seconds % 60uL).toString().padStart(2, '0')}"
        }
        SleepTimer.EndOfTrack -> "Pauses at the end of this song"
        SleepTimer.EndOfQueue -> "Pauses at the end of the queue"
        null -> "No timer set"
    }
