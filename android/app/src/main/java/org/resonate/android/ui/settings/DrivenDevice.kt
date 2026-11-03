package org.resonate.android.ui.settings

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Pause
import androidx.compose.material.icons.rounded.PlayArrow
import androidx.compose.material.icons.rounded.SkipNext
import androidx.compose.material.icons.rounded.SkipPrevious
import androidx.compose.material.icons.automirrored.rounded.VolumeDown
import androidx.compose.material.icons.automirrored.rounded.VolumeUp
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.produceState
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext
import org.resonate.android.nativecore.RemoteAnswer
import org.resonate.android.nativecore.RemoteControl
import org.resonate.android.nativecore.RemoteDevice
import org.resonate.android.nativecore.RemotePlaying
import org.resonate.android.nativecore.RemoteStanding
import org.resonate.android.nativecore.RemoteZoneState
import org.resonate.android.ui.Resonate
import org.resonate.android.ui.components.Sizes

const val DRIVEN_READ_EVERY_MS = 500L
const val VOLUME_STEP: UShort = 100u
const val WHOLE_VOLUME: UShort = 1000u

object DrivenWords {
    fun clock(millis: ULong): String {
        val seconds = (millis / 1000u).toLong()
        val hours = seconds / 3600
        val minutes = seconds / 60 % 60
        val rest = seconds % 60
        val twoDigits = { figure: Long -> figure.toString().padStart(2, '0') }
        return if (hours > 0) "$hours:${twoDigits(minutes)}:${twoDigits(rest)}"
        else "$minutes:${twoDigits(rest)}"
    }

    fun place(zone: RemoteZoneState): String {
        val at = clock(zone.positionMillis)
        return zone.lengthMillis?.let { "$at / ${clock(it)}" } ?: at
    }

    fun song(zone: RemoteZoneState): String {
        val playing = zone.playingRow ?: return "Nothing playing"
        val index = playing.toLong() - zone.firstRow.toLong()
        val row = zone.rows.getOrNull(index.toInt()) ?: return "Nothing playing"
        return if (row.artist.isBlank()) row.title else "${row.title} — ${row.artist}"
    }

    fun volume(zone: RemoteZoneState): String = "${(zone.volumePermille.toInt() + 5) / 10}%"

    fun louder(zone: RemoteZoneState): UShort =
        minOf(WHOLE_VOLUME.toInt(), zone.volumePermille.toInt() + VOLUME_STEP.toInt()).toUShort()

    fun quieter(zone: RemoteZoneState): UShort =
        maxOf(0, zone.volumePermille.toInt() - VOLUME_STEP.toInt()).toUShort()

    fun refusal(answer: RemoteAnswer): String? =
        when (answer) {
            RemoteAnswer.DONE -> null
            RemoteAnswer.UNTRUSTED -> "That device does not let others do that"
            RemoteAnswer.UNCONTROLLABLE -> "That device plays nothing another can drive"
            RemoteAnswer.NO_SUCH_ZONE -> "That zone is gone"
            RemoteAnswer.NO_SUCH_ROW -> "That song is no longer in its queue"
            RemoteAnswer.FAILED -> "The other device could not do that"
            RemoteAnswer.UNANSWERED -> "The other device did not answer"
        }
}

@Composable
fun DrivenDevice(resonate: Resonate, device: String, press: (UInt, RemoteControl) -> Unit) {
    val shown by
        produceState<RemoteDevice?>(null, device) {
            try {
                while (true) {
                    value = resonate.read { native -> withContext(Dispatchers.IO) { native.syncWatch(device) } }.getOrNull()
                    delay(DRIVEN_READ_EVERY_MS)
                }
            } finally {
                withContext(NonCancellable + Dispatchers.IO) { resonate.read { native -> native.syncUnwatch(device) } }
            }
        }
    Column(
        Modifier.fillMaxWidth().padding(horizontal = Sizes.gutter, vertical = 4.dp).testTag("driven-device"),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        val now = shown
        when {
            now == null || now.standing == RemoteStanding.WAITING ->
                Note("Asking the device what it plays…")
            now.standing == RemoteStanding.REFUSED ->
                Note("This device does not let others drive it, or cannot be driven")
            now.zones.isEmpty() -> Note("Asking the device what it plays…")
            else -> now.zones.forEach { zone -> DrivenZone(zone) { control -> press(zone.zone, control) } }
        }
    }
}

@Composable
private fun DrivenZone(zone: RemoteZoneState, press: (RemoteControl) -> Unit) {
    val sounding = zone.playing == RemotePlaying.PLAYING
    Surface(shape = MaterialTheme.shapes.large, color = MaterialTheme.colorScheme.surfaceContainerHigh) {
        Column(Modifier.fillMaxWidth().padding(12.dp)) {
            Text(
                "${zone.name} · ${DrivenWords.volume(zone)}",
                style = MaterialTheme.typography.labelMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Text(
                DrivenWords.song(zone),
                style = MaterialTheme.typography.bodyLarge,
                maxLines = 2,
                modifier = Modifier.semantics { liveRegion = LiveRegionMode.Polite },
            )
            Text(DrivenWords.place(zone), style = MaterialTheme.typography.bodySmall)
            Row(verticalAlignment = Alignment.CenterVertically) {
                IconButton(onClick = { press(RemoteControl.Previous) }) {
                    Icon(Icons.Rounded.SkipPrevious, contentDescription = "Previous song")
                }
                IconButton(onClick = { press(RemoteControl.Toggle) }) {
                    Icon(
                        if (sounding) Icons.Rounded.Pause else Icons.Rounded.PlayArrow,
                        contentDescription = if (sounding) "Pause" else "Play",
                    )
                }
                IconButton(onClick = { press(RemoteControl.Next) }) {
                    Icon(Icons.Rounded.SkipNext, contentDescription = "Next song")
                }
                IconButton(onClick = { press(RemoteControl.Volume(DrivenWords.quieter(zone))) }) {
                    Icon(Icons.AutoMirrored.Rounded.VolumeDown, contentDescription = "Quieter")
                }
                IconButton(onClick = { press(RemoteControl.Volume(DrivenWords.louder(zone))) }) {
                    Icon(Icons.AutoMirrored.Rounded.VolumeUp, contentDescription = "Louder")
                }
            }
        }
    }
}
