package org.resonate.android.ui.equaliser

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.AudioFile
import androidx.compose.material.icons.rounded.DeleteOutline
import androidx.compose.material.icons.rounded.Headphones
import androidx.compose.material.icons.rounded.SurroundSound
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.resonate.android.ui.components.SectionHeader
import org.resonate.android.ui.components.Sizes

private const val AUDIO_DOCUMENTS = "audio/*"

@Composable
fun RoomCorrectionSection(model: EqualiserViewModel) {
    val room by model.room.collectAsStateWithLifecycle()
    var forgetting by rememberSaveable { mutableStateOf(false) }
    val leftPicker =
        rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
            if (uri != null) model.chooseRoomLeft(uri)
        }
    val rightPicker =
        rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
            if (uri != null) model.chooseRoomRight(uri)
        }
    val named = room.name != null
    val left = room.left
    Column {
        SectionHeader("Room correction")
        Panel {
            ListItem(
                checked = room.on,
                onCheckedChange = model::turnRoom,
                enabled = named,
                modifier = Modifier.testTag("room-switch"),
                colors = ListItemDefaults.colors(containerColor = Color.Transparent),
                leadingContent = { Icon(Icons.Rounded.SurroundSound, null) },
                supportingContent = {
                    Text(
                        if (named) listOfNotNull(room.name, describeRoom(room.kept).ifEmpty { null }).joinToString(" · ")
                        else "Choose a measured response first"
                    )
                },
                trailingContent = { Switch(checked = room.on, onCheckedChange = null, enabled = named) },
            ) { Text("Correct for the room") }
            if (left != null) {
                ListItem(
                    modifier = Modifier.testTag("room-pair-left"),
                    colors = ListItemDefaults.colors(containerColor = Color.Transparent),
                    leadingContent = { Icon(Icons.Rounded.Headphones, null) },
                    supportingContent = { Text(left.name) },
                ) { Text("Left speaker") }
            }
        }
        if (left != null) {
            FlowRow(
                Modifier.fillMaxWidth().padding(horizontal = Sizes.gutter, vertical = 8.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                FilledTonalButton(
                    onClick = { rightPicker.launch(arrayOf(AUDIO_DOCUMENTS)) },
                    enabled = !room.reading,
                    contentPadding = ButtonDefaults.ButtonWithIconContentPadding,
                    modifier = Modifier.testTag("room-pair-right"),
                ) {
                    Icon(Icons.Rounded.AudioFile, null, Modifier.size(ButtonDefaults.IconSize))
                    Spacer(Modifier.width(ButtonDefaults.IconSpacing))
                    Text(if (room.reading) "Reading…" else "Choose the right speaker's response")
                }
                TextButton(onClick = model::cancelRoomPair, enabled = !room.reading) { Text("Cancel") }
            }
        } else {
            FlowRow(
                Modifier.fillMaxWidth().padding(horizontal = Sizes.gutter, vertical = 8.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                FilledTonalButton(
                    onClick = model::chooseRoomResponse,
                    enabled = !room.reading,
                    contentPadding = ButtonDefaults.ButtonWithIconContentPadding,
                    modifier = Modifier.testTag("room-choose"),
                ) {
                    Icon(Icons.Rounded.AudioFile, null, Modifier.size(ButtonDefaults.IconSize))
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
                    onClick = { leftPicker.launch(arrayOf(AUDIO_DOCUMENTS)) },
                    enabled = !room.reading,
                    contentPadding = ButtonDefaults.ButtonWithIconContentPadding,
                    modifier = Modifier.testTag("room-choose-pair"),
                ) {
                    Icon(Icons.Rounded.Headphones, null, Modifier.size(ButtonDefaults.IconSize))
                    Spacer(Modifier.width(ButtonDefaults.IconSpacing))
                    Text("Choose a pair")
                }
                if (named) {
                    OutlinedButton(
                        onClick = { forgetting = true },
                        contentPadding = ButtonDefaults.ButtonWithIconContentPadding,
                        colors = ButtonDefaults.outlinedButtonColors(contentColor = MaterialTheme.colorScheme.error),
                    ) {
                        Icon(Icons.Rounded.DeleteOutline, null, Modifier.size(ButtonDefaults.IconSize))
                        Spacer(Modifier.width(ButtonDefaults.IconSpacing))
                        Text("Forget")
                    }
                }
            }
        }
        Text(
            "A measured impulse response, up to ten seconds long, is convolved before the equaliser on every output. It is lowered where it would lift the music past full scale. A four-channel file is a two-by-two (binaural) response, in the order left-to-left, left-to-right, right-to-left, right-to-right. A pair is two stereo responses, the left speaker's and then the right's, each as heard at both ears, joined into one two-by-two response.",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(horizontal = Sizes.gutter, vertical = 4.dp),
        )
    }
    if (forgetting) {
        AlertDialog(
            onDismissRequest = { forgetting = false },
            icon = { Icon(Icons.Rounded.DeleteOutline, null) },
            title = { Text("Forget the room response?") },
            text = { Text("Room correction turns off and the kept response is deleted. You can choose it again later.") },
            confirmButton = {
                TextButton(
                    onClick = {
                        forgetting = false
                        model.forgetRoom()
                    },
                    colors = ButtonDefaults.textButtonColors(contentColor = MaterialTheme.colorScheme.error),
                ) { Text("Forget") }
            },
            dismissButton = { TextButton(onClick = { forgetting = false }) { Text("Cancel") } },
        )
    }
}
