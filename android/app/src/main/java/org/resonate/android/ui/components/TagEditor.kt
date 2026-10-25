package org.resonate.android.ui.components

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Close
import androidx.compose.material.icons.automirrored.rounded.Undo
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearWavyProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshots.SnapshotStateMap
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.testTagsAsResourceId
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.CancellationException
import org.resonate.android.TagDiff
import org.resonate.android.TagName
import org.resonate.android.TagSentences
import org.resonate.android.nativecore.Track
import org.resonate.android.ui.LocalResonate

@Composable
fun TagEditorSheet(tracks: List<Track>, dismiss: () -> Unit) {
    val resonate = LocalResonate.current
    val features = resonate.features
    val writable = remember(tracks) { tracks.filter(features.tags::writable) }
    val shown = remember { mutableStateMapOf<TagName, String>() }
    var read by remember { mutableStateOf<Map<TagName, String>>(emptyMap()) }
    var mixed by remember { mutableStateOf<Set<TagName>>(emptySet()) }
    var emptied by remember { mutableStateOf<Set<TagName>>(emptySet()) }
    var loaded by remember { mutableStateOf(false) }
    var failure by remember { mutableStateOf<String?>(null) }
    var discarding by remember { mutableStateOf(false) }
    var attempt by remember { mutableIntStateOf(0) }

    LaunchedEffect(writable, attempt) {
        if (writable.isEmpty()) return@LaunchedEffect
        try {
            val all =
                features.tags.read(writable.take(TagDiff.SONGS_READ_AT_MOST).map { it.id })
            val shared = TagDiff.common(all)
            read = shared.common
            mixed = shared.differing
            shown.clear()
            shown.putAll(shared.common)
            failure = null
            loaded = true
        } catch (cancelled: CancellationException) {
            throw cancelled
        } catch (error: Exception) {
            failure = error.message ?: "The tags could not be read"
        }
    }

    val changed = TagDiff.changed(shown, read, mixed, emptied)
    val request = { if (changed.isEmpty()) dismiss() else discarding = true }

    ModalBottomSheet(onDismissRequest = request) {
        Column(
            Modifier.verticalScroll(rememberScrollState())
                .imePadding()
                .navigationBarsPadding()
                .padding(horizontal = 24.dp)
                .semantics { testTagsAsResourceId = true },
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Text(
                TagSentences.editTitle(tracks.size),
                style = MaterialTheme.typography.titleLarge,
            )
            when {
                writable.isEmpty() ->
                    Text(
                        "Tags of this kind of file cannot be written.",
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                failure != null -> {
                    Text(failure.orEmpty(), color = MaterialTheme.colorScheme.error)
                    OutlinedButton(onClick = { attempt++ }) { Text("Try again") }
                }
                !loaded -> SmallLoadingRow()
                else -> {
                    val names = TagName.entries.filter { tracks.size == 1 || !it.eachSongsOwn }
                    names.filter { !it.numbered }.forEach { name ->
                        TagField(name, shown, mixed, emptied) { emptied = it }
                    }
                    if (tracks.size > 1) {
                        Text(
                            "Fields left as they are stay as each song has them; a field marked Mixed changes only once you type into it.",
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                    NumberRow(TagName.TrackNumber, TagName.TrackTotal, names, shown, mixed, emptied) {
                        emptied = it
                    }
                    NumberRow(TagName.DiscNumber, TagName.DiscTotal, names, shown, mixed, emptied) {
                        emptied = it
                    }
                    if (writable.size < tracks.size) {
                        Text(
                            TagSentences.cannotBeWritten(tracks.size - writable.size),
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
            }
            Row(
                Modifier.fillMaxWidth().padding(bottom = 16.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.End),
            ) {
                TextButton(onClick = request, modifier = Modifier.testTag("tags-cancel")) {
                    Text("Cancel")
                }
                Button(
                    onClick = {
                        resonate.editTags(writable, TagDiff.change(shown, changed))
                        dismiss()
                    },
                    enabled = loaded && writable.isNotEmpty() && changed.isNotEmpty(),
                    modifier = Modifier.testTag("tags-save"),
                ) {
                    Text("Save")
                }
            }
        }
    }

    if (discarding) {
        AlertDialog(
            onDismissRequest = { discarding = false },
            icon = { Icon(Icons.AutoMirrored.Rounded.Undo, contentDescription = null) },
            title = { Text("Discard your changes?") },
            text = { Text("The tags you typed have not been saved.") },
            confirmButton = {
                TextButton(onClick = dismiss, modifier = Modifier.testTag("tags-discard")) {
                    Text("Discard")
                }
            },
            dismissButton = { TextButton(onClick = { discarding = false }) { Text("Keep editing") } },
        )
    }
}

@Composable
private fun SmallLoadingRow() {
    LinearWavyProgressIndicator(Modifier.fillMaxWidth())
}

@Composable
private fun NumberRow(
    number: TagName,
    total: TagName,
    names: List<TagName>,
    shown: SnapshotStateMap<TagName, String>,
    mixed: Set<TagName>,
    emptied: Set<TagName>,
    emptying: (Set<TagName>) -> Unit,
) {
    Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        if (number in names) {
            TagField(number, shown, mixed, emptied, Modifier.weight(1f), emptying)
        }
        TagField(total, shown, mixed, emptied, Modifier.weight(1f), emptying)
    }
}

@Composable
private fun TagField(
    name: TagName,
    shown: SnapshotStateMap<TagName, String>,
    mixed: Set<TagName>,
    emptied: Set<TagName>,
    modifier: Modifier = Modifier,
    emptying: (Set<TagName>) -> Unit,
) {
    val value = shown[name].orEmpty()
    val clearing = name in emptied
    val label =
        when (name) {
            TagName.TrackTotal -> "Track of"
            TagName.DiscTotal -> "Disc of"
            else -> name.label
        }
    OutlinedTextField(
        value = value,
        onValueChange = {
            shown[name] = it
            if (it.isNotEmpty() && clearing) emptying(emptied - name)
        },
        label = { Text(label) },
        placeholder = {
            if (clearing) Text("Will be cleared") else if (name in mixed) Text("Mixed")
        },
        trailingIcon =
            if (name in mixed && value.isEmpty()) {
                {
                    IconButton(
                        onClick = { emptying(if (clearing) emptied - name else emptied + name) }
                    ) {
                        Icon(
                            if (clearing) Icons.AutoMirrored.Rounded.Undo else Icons.Rounded.Close,
                            contentDescription =
                                if (clearing) "Keep each song's" else "Clear on every song",
                        )
                    }
                }
            } else null,
        singleLine = true,
        keyboardOptions =
            KeyboardOptions(
                keyboardType = if (name.numbered) KeyboardType.Number else KeyboardType.Text,
                capitalization =
                    if (name.numbered) KeyboardCapitalization.None else KeyboardCapitalization.Words,
                autoCorrectEnabled = false,
            ),
        modifier = modifier.fillMaxWidth().testTag("tag-${name.name}"),
    )
}
