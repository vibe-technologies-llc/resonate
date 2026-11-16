package org.resonate.android.ui.library

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.selection.toggleable
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.CheckCircle
import androidx.compose.material.icons.rounded.HighQuality
import androidx.compose.material3.Button
import androidx.compose.material3.Checkbox
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.SheetValue
import androidx.compose.material3.Text
import androidx.compose.material3.rememberBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.testTagsAsResourceId
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.resonate.android.LossySongs
import org.resonate.android.R
import org.resonate.android.nativecore.LossySong
import org.resonate.android.ui.LocalResonate
import org.resonate.android.ui.components.Artwork
import org.resonate.android.ui.components.EmptyState
import org.resonate.android.ui.components.Sizes
import org.resonate.android.ui.settings.ConfirmDialog
import org.resonate.android.ui.shownTitle
import org.resonate.android.words

@Composable
fun LossySongsSheet(playlist: ULong, name: String, onDismiss: () -> Unit) {
    val resonate = LocalResonate.current
    val words = words()
    val state = rememberBottomSheetState(SheetValue.Hidden, enabledValues = setOf(SheetValue.Hidden, SheetValue.Expanded))
    val scope = rememberCoroutineScope()
    var found by remember { mutableStateOf<List<LossySong>?>(null) }
    var failure by remember { mutableStateOf<String?>(null) }
    var ticked by remember { mutableStateOf(emptySet<ULong>()) }
    var asked by remember { mutableIntStateOf(0) }
    var confirming by remember { mutableStateOf(false) }
    var replacing by remember { mutableStateOf(false) }
    LaunchedEffect(asked) {
        resonate
            .read { native -> withContext(Dispatchers.IO) { native.lossySongsOf(playlist) } }
            .onSuccess { songs ->
                ticked = if (found == null) LossySongs.ticked(songs) else LossySongs.stillTicked(songs, ticked)
                found = songs
                failure = null
            }
            .onFailure { failure = it.message ?: words.string(R.string.lossy_failed) }
    }
    val songs = found.orEmpty()
    val deletes = LossySongs.deletes(songs, ticked)
    val replace: () -> Unit = {
        replacing = true
        scope.launch {
            try {
                resonate.replaceLossySongs(ticked.toList())
            } catch (failed: Exception) {
                resonate.library.report(failed)
            } finally {
                replacing = false
                asked++
            }
        }
    }
    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = state) {
        Column(
            Modifier.fillMaxHeight(0.92f)
                .navigationBarsPadding()
                .semantics { testTagsAsResourceId = true }
                .testTag("lossy-songs")
        ) {
            Column(Modifier.fillMaxWidth().padding(horizontal = 24.dp)) {
                Text(
                    stringResource(R.string.lossy_title, name),
                    style = MaterialTheme.typography.titleLarge,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.semantics { heading() },
                )
                Text(
                    stringResource(R.string.lossy_detail),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            val shown = found
            when {
                failure != null ->
                    EmptyState(
                        Icons.Rounded.HighQuality,
                        stringResource(R.string.lossy_failed),
                        modifier = Modifier.weight(1f),
                        detail = failure,
                        action = stringResource(R.string.identical_try_again),
                        onAction = { asked++ },
                    )
                shown == null ->
                    Box(Modifier.weight(1f).fillMaxWidth(), contentAlignment = Alignment.Center) {
                        CircularProgressIndicator()
                    }
                shown.isEmpty() ->
                    EmptyState(
                        Icons.Rounded.CheckCircle,
                        stringResource(R.string.lossy_none),
                        modifier = Modifier.weight(1f),
                        detail = stringResource(R.string.lossy_none_detail),
                    )
                else ->
                    LazyColumn(Modifier.weight(1f).testTag("lossy-list")) {
                        items(shown, key = { it.song.id.toString() }) { lossy ->
                            val tickable = LossySongs.tickable(lossy)
                            val picked = lossy.song.id in ticked
                            ListItem(
                                modifier =
                                    Modifier.background(
                                            if (picked) MaterialTheme.colorScheme.secondaryContainer
                                            else Color.Transparent
                                        )
                                        .toggleable(value = picked, enabled = tickable && !replacing, role = Role.Checkbox) { on ->
                                            ticked = if (on) ticked + lossy.song.id else ticked - lossy.song.id
                                        },
                                colors = ListItemDefaults.colors(containerColor = Color.Transparent),
                                leadingContent = {
                                    Artwork(lossy.song, Sizes.rowArt, shape = MaterialTheme.shapes.medium)
                                },
                                supportingContent = {
                                    Text(LossySongs.line(words, lossy), maxLines = 2, overflow = TextOverflow.Ellipsis)
                                },
                                trailingContent = {
                                    Checkbox(checked = picked, onCheckedChange = null, enabled = tickable)
                                },
                            ) {
                                Text(lossy.song.shownTitle, maxLines = 1, overflow = TextOverflow.Ellipsis)
                            }
                        }
                    }
            }
            Button(
                onClick = { if (deletes > 0) confirming = true else replace() },
                enabled = ticked.isNotEmpty() && !replacing,
                modifier = Modifier.fillMaxWidth().padding(Sizes.gutter).testTag("replace-lossy"),
            ) {
                Text(
                    if (ticked.isEmpty()) stringResource(R.string.lossy_replace)
                    else pluralStringResource(R.plurals.lossy_replace_count, ticked.size, ticked.size)
                )
            }
        }
    }
    if (confirming) {
        ConfirmDialog(
            title = pluralStringResource(R.plurals.lossy_delete_title, deletes, deletes),
            text = stringResource(R.string.lossy_delete_text),
            confirm = pluralStringResource(R.plurals.lossy_replace_count, ticked.size, ticked.size),
            icon = Icons.Rounded.HighQuality,
            destructive = true,
            onConfirm = replace,
            onDismiss = { confirming = false },
        )
    }
}
