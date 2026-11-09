package org.resonate.android.ui.equaliser

import android.net.Uri
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.RotateLeft
import androidx.compose.material.icons.rounded.Add
import androidx.compose.material.icons.rounded.AudioFile
import androidx.compose.material.icons.rounded.DeleteOutline
import androidx.compose.material.icons.rounded.Headphones
import androidx.compose.material.icons.rounded.Height
import androidx.compose.material.icons.rounded.Remove
import androidx.compose.material.icons.rounded.SpeakerGroup
import androidx.compose.material.icons.rounded.SurroundSound
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import kotlinx.coroutines.flow.StateFlow
import org.resonate.android.RESPONSE_DOCUMENTS
import org.resonate.android.RoomView
import org.resonate.android.nativecore.SpeakerLimits
import org.resonate.android.ui.Resonate
import org.resonate.android.ui.components.Page
import org.resonate.android.ui.components.SectionHeader
import org.resonate.android.ui.components.Sizes
import org.resonate.android.ui.resonateViewModel
import org.resonate.android.ui.settings.ButtonRow
import org.resonate.android.ui.settings.ConfirmDialog
import org.resonate.android.ui.settings.Note
import org.resonate.android.ui.settings.SettingRow
import org.resonate.android.ui.settings.SettingsList
import org.resonate.android.ui.settings.SwitchRow

val ANGLE_PRESETS = listOf(20, 30, 45, 60, 90)

val ELEVATION_PRESETS = listOf(0, 10, 20, 30, 45)

val YAW_PRESETS = listOf(-45, -20, 0, 20, 45)

class RoomCorrectionViewModel(private val resonate: Resonate) : ViewModel() {
    private val room = resonate.features.room

    val view: StateFlow<RoomView> = room.view

    val limits: SpeakerLimits = room.limits

    fun choose() = resonate.host.chooseRoomResponse()

    fun chooseLeft(uri: Uri) = room.chooseLeft(uri)

    fun chooseRight(uri: Uri) = room.chooseRight(uri)

    fun cancelPair() = room.cancelPair()

    fun turn(on: Boolean) = room.turn(on)

    fun forget() = room.forget()

    fun speakAt(angle: Int) = room.speakAt(angle)

    fun raiseTo(elevation: Int) = room.raiseTo(elevation)

    fun turnTo(yaw: Int) = room.turnTo(yaw)
}

@Composable
fun RoomCorrectionScreen() {
    val model = resonateViewModel { RoomCorrectionViewModel(it) }
    val room by model.view.collectAsStateWithLifecycle()
    var forgetting by rememberSaveable { mutableStateOf(false) }
    val leftPicker =
        rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
            if (uri != null) model.chooseLeft(uri)
        }
    val rightPicker =
        rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
            if (uri != null) model.chooseRight(uri)
        }
    val named = room.name != null
    val left = room.left
    val limits = model.limits
    Page("Room correction") { padding ->
        SettingsList(padding, Modifier.testTag("room-list")) {
            item(key = "switch") {
                SwitchRow(
                    title = "Correct for the room",
                    checked = room.on,
                    onCheckedChange = model::turn,
                    enabled = named,
                    icon = Icons.Rounded.SurroundSound,
                    supporting =
                        if (named) {
                            listOfNotNull(room.name, describeRoom(room.kept).ifEmpty { null })
                                .joinToString(" · ")
                        } else "Choose a measured response first",
                    modifier = Modifier.testTag("room-switch"),
                )
            }
            item(key = "response") {
                SectionHeader("The response")
                if (left != null) {
                    SettingRow(
                        title = "Left speaker",
                        icon = Icons.Rounded.Headphones,
                        supporting = left.name,
                        modifier = Modifier.testTag("room-pair-left"),
                    )
                    ButtonRow(start = true) {
                        FilledTonalButton(
                            onClick = { rightPicker.launch(RESPONSE_DOCUMENTS) },
                            enabled = !room.reading,
                            contentPadding = ButtonDefaults.ButtonWithIconContentPadding,
                            modifier = Modifier.testTag("room-pair-right"),
                        ) {
                            Icon(
                                Icons.Rounded.AudioFile,
                                null,
                                Modifier.size(ButtonDefaults.IconSize),
                            )
                            Spacer(Modifier.width(ButtonDefaults.IconSpacing))
                            Text(
                                if (room.reading) "Reading…"
                                else "Choose the right speaker's response"
                            )
                        }
                        TextButton(onClick = model::cancelPair, enabled = !room.reading) {
                            Text("Cancel")
                        }
                    }
                } else {
                    ButtonRow(start = true) {
                        FilledTonalButton(
                            onClick = model::choose,
                            enabled = !room.reading,
                            contentPadding = ButtonDefaults.ButtonWithIconContentPadding,
                            modifier = Modifier.testTag("room-choose"),
                        ) {
                            Icon(
                                Icons.Rounded.AudioFile,
                                null,
                                Modifier.size(ButtonDefaults.IconSize),
                            )
                            Spacer(Modifier.width(ButtonDefaults.IconSpacing))
                            Text(
                                when {
                                    room.reading -> "Reading…"
                                    named -> "Choose another response"
                                    else -> "Choose a response"
                                }
                            )
                        }
                        OutlinedButton(
                            onClick = { leftPicker.launch(RESPONSE_DOCUMENTS) },
                            enabled = !room.reading,
                            contentPadding = ButtonDefaults.ButtonWithIconContentPadding,
                            modifier = Modifier.testTag("room-choose-pair"),
                        ) {
                            Icon(
                                Icons.Rounded.Headphones,
                                null,
                                Modifier.size(ButtonDefaults.IconSize),
                            )
                            Spacer(Modifier.width(ButtonDefaults.IconSpacing))
                            Text("Choose a pair")
                        }
                        if (named) {
                            OutlinedButton(
                                onClick = { forgetting = true },
                                contentPadding = ButtonDefaults.ButtonWithIconContentPadding,
                                colors =
                                    ButtonDefaults.outlinedButtonColors(
                                        contentColor = MaterialTheme.colorScheme.error
                                    ),
                                modifier = Modifier.testTag("room-forget"),
                            ) {
                                Icon(
                                    Icons.Rounded.DeleteOutline,
                                    null,
                                    Modifier.size(ButtonDefaults.IconSize),
                                )
                                Spacer(Modifier.width(ButtonDefaults.IconSpacing))
                                Text("Forget")
                            }
                        }
                    }
                }
                Note(
                    "A measured impulse response, up to ten seconds long, played before the equaliser on every output and lowered where it would lift the music past full scale."
                )
                Note(
                    "A four-channel file is a two-by-two (binaural) response: left to left, left to right, right to left, right to right. A pair is the left speaker's stereo response and then the right's, joined into one."
                )
            }
            if (named) {
                item(key = "speakers") {
                    SectionHeader("The speakers")
                    Note(
                        "A SOFA set of head-related responses is read where the speakers stand and your head turns, blended where several measurements surround them."
                    )
                    SpeakerSetting(
                        title = "Speaker angle",
                        icon = Icons.Rounded.SpeakerGroup,
                        value = room.angle,
                        least = limits.angleLeastDegrees.toInt(),
                        most = limits.angleMostDegrees.toInt(),
                        presets = ANGLE_PRESETS,
                        lessSaying = "Bring the speakers a degree closer",
                        moreSaying = "Put the speakers a degree further apart",
                        tag = "speaker-angle",
                        onChange = model::speakAt,
                    )
                    SpeakerSetting(
                        title = "Speaker elevation",
                        icon = Icons.Rounded.Height,
                        value = room.elevation,
                        least = limits.elevationLeastDegrees.toInt(),
                        most = limits.elevationMostDegrees.toInt(),
                        presets = ELEVATION_PRESETS,
                        lessSaying = "Lower the speakers a degree",
                        moreSaying = "Raise the speakers a degree",
                        tag = "speaker-elevation",
                        onChange = model::raiseTo,
                    )
                    SpeakerSetting(
                        title = "Head turned",
                        icon = Icons.AutoMirrored.Rounded.RotateLeft,
                        value = room.yaw,
                        least = limits.yawLeastDegrees.toInt(),
                        most = limits.yawMostDegrees.toInt(),
                        presets = YAW_PRESETS,
                        lessSaying = "Turn the head a degree to the right",
                        moreSaying = "Turn the head a degree to the left",
                        tag = "head-yaw",
                        onChange = model::turnTo,
                    )
                }
            }
        }
    }
    if (forgetting) {
        ConfirmDialog(
            title = "Forget the room response?",
            text =
                "Room correction turns off and the kept response is deleted. You can choose it again later.",
            confirm = "Forget",
            onConfirm = model::forget,
            onDismiss = { forgetting = false },
            icon = Icons.Rounded.DeleteOutline,
            destructive = true,
        )
    }
}

@Composable
private fun SpeakerSetting(
    title: String,
    icon: ImageVector,
    value: Int,
    least: Int,
    most: Int,
    presets: List<Int>,
    lessSaying: String,
    moreSaying: String,
    tag: String,
    onChange: (Int) -> Unit,
) {
    SettingRow(
        title = title,
        icon = icon,
        supporting = "$value°",
        trailing = {
            Row {
                IconButton(
                    onClick = { onChange(value - 1) },
                    enabled = value > least,
                    modifier = Modifier.testTag("$tag-less"),
                ) {
                    Icon(Icons.Rounded.Remove, lessSaying)
                }
                IconButton(
                    onClick = { onChange(value + 1) },
                    enabled = value < most,
                    modifier = Modifier.testTag("$tag-more"),
                ) {
                    Icon(Icons.Rounded.Add, moreSaying)
                }
            }
        },
        modifier = Modifier.testTag(tag),
    )
    FlowRow(
        Modifier.fillMaxWidth().padding(start = Sizes.gutter, end = Sizes.gutter, bottom = 8.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        presets.forEach { preset ->
            FilterChip(
                selected = value == preset,
                onClick = { onChange(preset) },
                label = { Text("$preset°") },
                modifier = Modifier.testTag("$tag-$preset"),
            )
        }
    }
}
