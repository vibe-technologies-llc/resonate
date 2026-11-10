package org.resonate.android.ui.settings

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.CheckCircle
import androidx.compose.material.icons.rounded.DeleteOutline
import androidx.compose.material.icons.rounded.Done
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.resonate.android.MusicDocuments
import org.resonate.android.R
import org.resonate.android.nativecore.IdenticalSongs
import org.resonate.android.nativecore.Track
import org.resonate.android.ui.Resonate
import org.resonate.android.ui.components.EmptyState
import org.resonate.android.ui.components.Page
import org.resonate.android.ui.components.SectionHeader
import org.resonate.android.ui.components.Sizes
import org.resonate.android.ui.components.TrackRow
import org.resonate.android.ui.resonateViewModel
import org.resonate.android.ui.shownTitle

sealed interface IdenticalState {
    data object Comparing : IdenticalState

    data class Failed(val text: String) : IdenticalState

    data class Found(val sets: List<IdenticalSongs>) : IdenticalState
}

fun keptAfterChoices(sets: List<IdenticalSongs>, chosen: Map<ULong, ULong>): List<IdenticalSongs> =
    sets.map { set ->
        val members = listOf(set.kept) + set.copies
        val keeping = chosen[set.kept.id] ?: set.kept.id
        IdenticalSongs(
            members.firstOrNull { it.id == keeping } ?: set.kept,
            members.filter { it.id != keeping },
        )
    }

fun choicesStillStanding(sets: List<IdenticalSongs>, chosen: Map<ULong, ULong>): Map<ULong, ULong> =
    chosen.filter { (kept, keeping) ->
        sets.any { set -> set.kept.id == kept && (set.kept.id == keeping || set.copies.any { it.id == keeping }) }
    }

class IdenticalSongsViewModel(private val resonate: Resonate) : ViewModel() {
    private val mutableState = MutableStateFlow<IdenticalState>(IdenticalState.Comparing)
    private val mutableChosen = MutableStateFlow<Map<ULong, ULong>>(emptyMap())
    private val mutableDeleting = MutableStateFlow(false)

    val state: StateFlow<IdenticalState> = mutableState.asStateFlow()
    val chosen: StateFlow<Map<ULong, ULong>> = mutableChosen.asStateFlow()
    val deleting: StateFlow<Boolean> = mutableDeleting.asStateFlow()
    private var finding: Job? = null

    init {
        find()
    }

    fun find() {
        finding?.cancel()
        mutableState.value = IdenticalState.Comparing
        finding = viewModelScope.launch {
            val found = resonate.read { native -> withContext(Dispatchers.IO) { native.identicalSongs() } }
            found.onSuccess { sets -> mutableChosen.value = choicesStillStanding(sets, mutableChosen.value) }
            mutableState.value =
                found.fold(
                    { IdenticalState.Found(it) },
                    {
                        IdenticalState.Failed(
                            it.message
                                ?: resonate.library.words.string(R.string.identical_failed)
                        )
                    },
                )
        }
    }

    fun choose(set: IdenticalSongs, track: Track) {
        mutableChosen.value = mutableChosen.value + (set.kept.id to track.id)
    }

    fun deleteCopies() {
        val found = (mutableState.value as? IdenticalState.Found)?.sets ?: return
        val sets = keptAfterChoices(found, mutableChosen.value)
        mutableDeleting.value = true
        viewModelScope.launch {
            try {
                resonate.deleteCopies(sets)
            } catch (failure: Exception) {
                resonate.library.report(failure)
            } finally {
                mutableDeleting.value = false
                find()
            }
        }
    }

    fun place(track: Track): String {
        val folder =
            resonate.features.folders.value.firstOrNull {
                track.uri.startsWith(MusicDocuments.prefix(it.uri))
            }
        val path = listOfNotNull(folder?.name, track.name).joinToString(" › ")
        return if (folder != null && !folder.writable) {
            resonate.library.words.string(R.string.identical_read_only, path)
        } else {
            path
        }
    }
}

@Composable
fun IdenticalSongsScreen() {
    val model = resonateViewModel { IdenticalSongsViewModel(it) }
    val state by model.state.collectAsStateWithLifecycle()
    val chosen by model.chosen.collectAsStateWithLifecycle()
    val deleting by model.deleting.collectAsStateWithLifecycle()
    var confirming by rememberSaveable { mutableStateOf(false) }
    val found = (state as? IdenticalState.Found)?.sets.orEmpty()
    val copies = keptAfterChoices(found, chosen).sumOf { it.copies.size }
    val keptWord = stringResource(R.string.identical_kept)
    val deletedWord = stringResource(R.string.identical_deleted)
    Page(
        stringResource(R.string.identical_title),
        actions = {
            if (copies > 0) {
                TextButton(
                    onClick = { confirming = true },
                    enabled = !deleting,
                    modifier = Modifier.testTag("delete-copies"),
                ) {
                    Text(
                        stringResource(R.string.identical_delete_copies),
                        color = MaterialTheme.colorScheme.error,
                    )
                }
            }
        },
    ) { padding ->
        when (val shown = state) {
            IdenticalState.Comparing ->
                Box(Modifier.fillMaxSize().padding(padding), contentAlignment = Alignment.Center) {
                    CircularProgressIndicator()
                }
            is IdenticalState.Failed ->
                Box(Modifier.fillMaxSize().padding(padding)) {
                    EmptyState(
                        Icons.Rounded.DeleteOutline,
                        stringResource(R.string.identical_failed),
                        detail = shown.text,
                        action = stringResource(R.string.identical_try_again),
                        onAction = model::find,
                    )
                }
            is IdenticalState.Found ->
                if (shown.sets.isEmpty()) {
                    Box(Modifier.fillMaxSize().padding(padding)) {
                        EmptyState(
                            Icons.Rounded.CheckCircle,
                            stringResource(R.string.identical_none),
                            detail = stringResource(R.string.identical_none_detail),
                        )
                    }
                } else {
                    SettingsList(padding, Modifier.testTag("identical-list")) {
                        shown.sets.forEach { set ->
                            val members = listOf(set.kept) + set.copies
                            val keeping = chosen[set.kept.id] ?: set.kept.id
                            item(key = "set-${set.kept.id}") {
                                SectionHeader(set.kept.shownTitle)
                            }
                            items(members, key = { "copy-${it.id}" }) { track ->
                                val kept = track.id == keeping
                                TrackRow(
                                    track,
                                    onClick = { model.choose(set, track) },
                                    supporting = model.place(track),
                                    modifier =
                                        Modifier.semantics {
                                            stateDescription = if (kept) keptWord else deletedWord
                                        },
                                    trailing = {
                                        Icon(
                                            if (kept) Icons.Rounded.Done else Icons.Rounded.DeleteOutline,
                                            contentDescription = null,
                                            tint =
                                                if (kept) MaterialTheme.colorScheme.primary
                                                else MaterialTheme.colorScheme.onSurfaceVariant,
                                        )
                                    },
                                )
                            }
                            item(key = "hint-${set.kept.id}") {
                                Text(
                                    stringResource(R.string.identical_hint),
                                    style = MaterialTheme.typography.bodySmall,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    modifier = Modifier.padding(horizontal = Sizes.gutter, vertical = 8.dp),
                                )
                            }
                        }
                    }
                }
        }
    }
    if (confirming) {
        ConfirmDialog(
            title = pluralStringResource(R.plurals.identical_delete_title, copies, copies),
            text = stringResource(R.string.identical_delete_text),
            confirm = pluralStringResource(R.plurals.identical_delete_confirm, copies, copies),
            icon = Icons.Rounded.DeleteOutline,
            destructive = true,
            onConfirm = model::deleteCopies,
            onDismiss = { confirming = false },
        )
    }
}
