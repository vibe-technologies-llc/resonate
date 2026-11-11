package org.resonate.android.ui.settings

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Close
import androidx.compose.material.icons.rounded.KeyboardArrowDown
import androidx.compose.material.icons.rounded.KeyboardArrowUp
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
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext
import org.resonate.android.R
import org.resonate.android.Words
import org.resonate.android.nativecore.RemoteAnswer
import org.resonate.android.nativecore.RemoteControl
import org.resonate.android.nativecore.RemoteDevice
import org.resonate.android.nativecore.RemotePlaying
import org.resonate.android.nativecore.RemoteSong
import org.resonate.android.nativecore.RemoteStanding
import org.resonate.android.nativecore.RemoteZoneState
import org.resonate.android.ui.Resonate
import org.resonate.android.ui.components.CommitSlider
import org.resonate.android.ui.components.Sizes
import org.resonate.android.words

const val DRIVEN_READ_EVERY_MS = 500L
const val VOLUME_STEP: UShort = 100u
const val WHOLE_VOLUME: UShort = 1000u
const val DRIVEN_ROWS_AT_FIRST = 12
const val DRIVEN_ROWS_MORE = 50
const val DRIVEN_ROWS_BEFORE_THE_ONE_PLAYING = 2

data class DrivenRow(val row: UInt, val song: RemoteSong, val playing: Boolean)

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

    fun song(words: Words, zone: RemoteZoneState): String {
        val nothing = words.string(R.string.driven_nothing_playing)
        val playing = zone.playingRow ?: return nothing
        val index = playing.toLong() - zone.firstRow.toLong()
        val row = zone.rows.getOrNull(index.toInt()) ?: return nothing
        return if (row.artist.isBlank()) row.title else "${row.title} — ${row.artist}"
    }

    fun volume(zone: RemoteZoneState): String = "${(zone.volumePermille.toInt() + 5) / 10}%"

    fun louder(zone: RemoteZoneState): UShort =
        minOf(WHOLE_VOLUME.toInt(), zone.volumePermille.toInt() + VOLUME_STEP.toInt()).toUShort()

    fun quieter(zone: RemoteZoneState): UShort =
        maxOf(0, zone.volumePermille.toInt() - VOLUME_STEP.toInt()).toUShort()

    fun billed(song: RemoteSong): String =
        if (song.artist.isBlank()) song.title else "${song.title} — ${song.artist}"

    fun rowsShown(zone: RemoteZoneState, most: Int): List<DrivenRow> {
        val playing = zone.playingRow?.toLong() ?: zone.firstRow.toLong()
        val from =
            maxOf(playing - DRIVEN_ROWS_BEFORE_THE_ONE_PLAYING, zone.firstRow.toLong())
        return zone.rows
            .mapIndexed { offset, song ->
                val row = zone.firstRow + offset.toUInt()
                DrivenRow(row, song, row == zone.playingRow)
            }
            .filter { it.row.toLong() >= from }
            .take(most)
    }

    fun rowsBefore(zone: RemoteZoneState): Int {
        val playing = zone.playingRow?.toLong() ?: zone.firstRow.toLong()
        return (playing - DRIVEN_ROWS_BEFORE_THE_ONE_PLAYING - zone.firstRow.toLong())
            .coerceIn(0L, zone.rows.size.toLong())
            .toInt()
    }

    fun moveUp(row: UInt): RemoteControl? =
        if (row == 0u) null else RemoteControl.Move(row, row - 1u)

    fun moveDown(row: UInt, queueLength: UInt): RemoteControl? =
        if (row + 1u < queueLength) RemoteControl.Move(row, row + 1u) else null

    fun share(zone: RemoteZoneState): Float {
        val length = zone.lengthMillis ?: return 0f
        if (length == 0uL) return 0f
        return (zone.positionMillis.toDouble() / length.toDouble()).toFloat().coerceIn(0f, 1f)
    }

    fun seekTo(zone: RemoteZoneState, share: Float): RemoteControl? {
        val length = zone.lengthMillis ?: return null
        return RemoteControl.Seek((length.toDouble() * share.coerceIn(0f, 1f)).toULong())
    }

    fun refusal(words: Words, answer: RemoteAnswer): String? =
        when (answer) {
            RemoteAnswer.DONE -> null
            RemoteAnswer.UNTRUSTED -> words.string(R.string.driven_refused_untrusted)
            RemoteAnswer.UNCONTROLLABLE -> words.string(R.string.driven_refused_uncontrollable)
            RemoteAnswer.NO_SUCH_ZONE -> words.string(R.string.driven_refused_no_zone)
            RemoteAnswer.NO_SUCH_ROW -> words.string(R.string.driven_refused_no_row)
            RemoteAnswer.FAILED -> words.string(R.string.driven_refused_failed)
            RemoteAnswer.UNANSWERED -> words.string(R.string.driven_refused_unanswered)
        }
}

@Composable
fun DrivenDevice(resonate: Resonate, device: String, press: (UInt, RemoteControl, ULong?) -> Unit) {
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
                Note(stringResource(R.string.driven_asking))
            now.standing == RemoteStanding.REFUSED ->
                Note(stringResource(R.string.driven_refused_device))
            now.zones.isEmpty() -> Note(stringResource(R.string.driven_asking))
            else -> now.zones.forEach { zone -> DrivenZone(zone) { control -> press(zone.zone, control, zone.queueRevision) } }
        }
    }
}

@Composable
private fun DrivenZone(zone: RemoteZoneState, press: (RemoteControl) -> Unit) {
    val sounding = zone.playing == RemotePlaying.PLAYING
    val words = words()
    Surface(shape = MaterialTheme.shapes.large, color = MaterialTheme.colorScheme.surfaceContainerHigh) {
        Column(Modifier.fillMaxWidth().padding(12.dp)) {
            Text(
                "${zone.name} · ${DrivenWords.volume(zone)}",
                style = MaterialTheme.typography.labelMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Text(
                DrivenWords.song(words, zone),
                style = MaterialTheme.typography.bodyLarge,
                maxLines = 2,
                modifier = Modifier.semantics { liveRegion = LiveRegionMode.Polite },
            )
            Text(DrivenWords.place(zone), style = MaterialTheme.typography.bodySmall)
            if ((zone.lengthMillis ?: 0uL) > 0uL) {
                CommitSlider(
                    value = DrivenWords.share(zone),
                    onCommit = { share -> DrivenWords.seekTo(zone, share)?.let(press) },
                    trackHeight = 4.dp,
                    stateText = { share ->
                        val at = (zone.lengthMillis ?: 0uL).toDouble() * share
                        words.string(
                            R.string.driven_seek_state,
                            DrivenWords.clock(at.toULong()),
                            DrivenWords.clock(zone.lengthMillis ?: 0uL),
                        )
                    },
                    modifier = Modifier.testTag("driven-seek-${zone.zone}"),
                )
            }
            Row(verticalAlignment = Alignment.CenterVertically) {
                IconButton(onClick = { press(RemoteControl.Previous) }) {
                    Icon(
                        Icons.Rounded.SkipPrevious,
                        contentDescription = stringResource(R.string.driven_previous),
                    )
                }
                IconButton(onClick = { press(RemoteControl.Toggle) }) {
                    Icon(
                        if (sounding) Icons.Rounded.Pause else Icons.Rounded.PlayArrow,
                        contentDescription =
                            stringResource(
                                if (sounding) R.string.driven_pause else R.string.driven_play
                            ),
                    )
                }
                IconButton(onClick = { press(RemoteControl.Next) }) {
                    Icon(
                        Icons.Rounded.SkipNext,
                        contentDescription = stringResource(R.string.driven_next),
                    )
                }
                IconButton(onClick = { press(RemoteControl.Volume(DrivenWords.quieter(zone))) }) {
                    Icon(
                        Icons.AutoMirrored.Rounded.VolumeDown,
                        contentDescription = stringResource(R.string.driven_quieter),
                    )
                }
                IconButton(onClick = { press(RemoteControl.Volume(DrivenWords.louder(zone))) }) {
                    Icon(
                        Icons.AutoMirrored.Rounded.VolumeUp,
                        contentDescription = stringResource(R.string.driven_louder),
                    )
                }
            }
            DrivenRows(zone, press)
        }
    }
}

@Composable
private fun DrivenRows(zone: RemoteZoneState, press: (RemoteControl) -> Unit) {
    var most by remember(zone.zone) { mutableIntStateOf(DRIVEN_ROWS_AT_FIRST) }
    val shown = DrivenWords.rowsShown(zone, most)
    shown.forEach { row -> DrivenQueueRow(zone, row, press) }
    if (zone.rows.size > shown.size + DrivenWords.rowsBefore(zone)) {
        TextButton(onClick = { most += DRIVEN_ROWS_MORE }) {
            Text(stringResource(R.string.driven_show_more))
        }
    }
}

@Composable
private fun DrivenQueueRow(zone: RemoteZoneState, row: DrivenRow, press: (RemoteControl) -> Unit) {
    val playingWord = stringResource(R.string.driven_playing)
    Row(verticalAlignment = Alignment.CenterVertically) {
        Text(
            DrivenWords.billed(row.song),
            style = MaterialTheme.typography.bodyMedium,
            color =
                if (row.playing) MaterialTheme.colorScheme.primary
                else MaterialTheme.colorScheme.onSurface,
            maxLines = 2,
            modifier =
                Modifier.weight(1f)
                    .clickable(
                        role = Role.Button,
                        onClickLabel = stringResource(R.string.driven_play_this),
                    ) {
                        press(RemoteControl.Jump(row.row))
                    }
                    .padding(vertical = 8.dp)
                    .semantics { if (row.playing) stateDescription = playingWord }
                    .testTag("driven-row-${zone.zone}-${row.row}"),
        )
        DrivenWords.moveUp(row.row)?.let { up ->
            IconButton(onClick = { press(up) }) {
                Icon(
                    Icons.Rounded.KeyboardArrowUp,
                    contentDescription = stringResource(R.string.driven_move_up, row.song.title),
                )
            }
        }
        DrivenWords.moveDown(row.row, zone.queueLength)?.let { down ->
            IconButton(onClick = { press(down) }) {
                Icon(
                    Icons.Rounded.KeyboardArrowDown,
                    contentDescription = stringResource(R.string.driven_move_down, row.song.title),
                )
            }
        }
        IconButton(onClick = { press(RemoteControl.Remove(row.row)) }) {
            Icon(
                Icons.Rounded.Close,
                contentDescription = stringResource(R.string.driven_remove, row.song.title),
            )
        }
    }
}
