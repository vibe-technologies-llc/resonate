package org.resonate.android.ui.settings

import android.net.Uri
import android.provider.DocumentsContract
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.DriveFileRenameOutline
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.KeyboardType
import androidx.lifecycle.ViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.launch
import org.resonate.android.OrganiseSentences
import org.resonate.android.R
import org.resonate.android.Words
import org.resonate.android.nativecore.FiledSong
import org.resonate.android.nativecore.OrganisePlan
import org.resonate.android.nativecore.defaultOrganiseLayout
import org.resonate.android.nativecore.organiseLayoutProblem
import org.resonate.android.ui.Resonate
import org.resonate.android.ui.components.Page
import org.resonate.android.ui.components.SectionHeader
import org.resonate.android.ui.navigation.LocalNavigator
import org.resonate.android.words
import org.resonate.android.ui.resonateViewModel

data class Planned(val layout: String, val plan: OrganisePlan)

object OrganiseWords {
    fun explained(words: Words, folder: String): String =
        words.string(R.string.organise_explained, folder)

    fun current(planned: Planned?, layout: String): OrganisePlan? =
        planned?.takeIf { it.layout == layout }?.plan

    fun waiting(problem: String?, current: OrganisePlan?, failedFor: String?, layout: String) =
        problem == null && current == null && failedFor != layout
}

class OrganiseViewModel(private val resonate: Resonate, private val folder: Uri) : ViewModel() {
    private val organising = resonate.features.organising
    private val mutablePlanned = MutableStateFlow<Planned?>(null)
    private val mutableFailedFor = MutableStateFlow<String?>(null)
    private val mutablePlanning = MutableStateFlow(false)

    val name: String =
        resonate.features.folders.value.firstOrNull { it.uri == folder }?.name
            ?: resonate.library.words.string(R.string.organise_this_folder)
    val rootId: String = DocumentsContract.getTreeDocumentId(folder)
    var layout by mutableStateOf(organising.layout())
        private set
    val planned: StateFlow<Planned?> = mutablePlanned.asStateFlow()
    val failedFor: StateFlow<String?> = mutableFailedFor.asStateFlow()
    val planning: StateFlow<Boolean> = mutablePlanning.asStateFlow()
    val busy = organising.busy

    init {
        viewModelScope.launch { snapshotFlow { layout }.collectLatest { preview(it) } }
    }

    fun type(text: String) {
        layout = text
    }

    fun organise(moves: List<FiledSong>) {
        organising.keepLayout(layout)
        organising.organise(folder, moves)
    }

    private suspend fun preview(asked: String) {
        if (organiseLayoutProblem(asked) != null) {
            mutablePlanned.value = null
            return
        }
        delay(PREVIEW_AFTER_MS)
        mutablePlanning.value = true
        try {
            mutablePlanned.value = Planned(asked, organising.plan(folder, asked))
            mutableFailedFor.value = null
        } catch (cancelled: CancellationException) {
            throw cancelled
        } catch (failure: Exception) {
            mutableFailedFor.value = asked
            resonate.library.report(failure)
        } finally {
            mutablePlanning.value = false
        }
    }

    companion object {
        const val PREVIEW_AFTER_MS = 300L
    }
}

@Composable
fun OrganiseScreen(folder: String) {
    val uri = remember(folder) { Uri.parse(folder) }
    val model = resonateViewModel(key = "organise:$folder") { OrganiseViewModel(it, uri) }
    val navigator = LocalNavigator.current
    val layout = model.layout
    val planned by model.planned.collectAsStateWithLifecycle()
    val failedFor by model.failedFor.collectAsStateWithLifecycle()
    val planning by model.planning.collectAsStateWithLifecycle()
    val busy by model.busy.collectAsStateWithLifecycle()
    var confirming by rememberSaveable { mutableStateOf(false) }
    val problem = remember(layout) { organiseLayoutProblem(layout) }
    val usual = remember { defaultOrganiseLayout() }
    val current = OrganiseWords.current(planned, layout)
    val waiting = OrganiseWords.waiting(problem, current, failedFor, layout)
    val moves = current?.moves.orEmpty()
    val words = words()
    Page(
        stringResource(R.string.organise_title, model.name),
        actions = {
            TextButton(
                onClick = { confirming = true },
                enabled = problem == null && !planning && moves.isNotEmpty() && !busy,
                modifier = Modifier.testTag("organise"),
            ) {
                Text(stringResource(R.string.organise_action))
            }
        },
    ) { padding ->
        SettingsList(padding, Modifier.testTag("organise-list")) {
            item(key = "layout") {
                Field(
                    value = layout,
                    onValueChange = model::type,
                    label = stringResource(R.string.organise_layout),
                    supporting = problem ?: stringResource(R.string.organise_fields),
                    error = problem != null,
                    keyboardType = KeyboardType.Uri,
                    modifier = Modifier.testTag("organise-layout"),
                )
            }
            if (layout != usual) {
                item(key = "usual") {
                    ButtonRow {
                        OutlinedButton(onClick = { model.type(usual) }) {
                            Text(stringResource(R.string.organise_usual_layout))
                        }
                    }
                }
            }
            if (planning || waiting) {
                item(key = "planning") { ListLoading() }
            } else if (current != null) {
                item(key = "preview") { SectionHeader(stringResource(R.string.organise_preview)) }
                item(key = "to-move") {
                    SettingRow(
                        stringResource(R.string.organise_to_move),
                        supporting =
                            pluralStringResource(R.plurals.count_songs, moves.size, moves.size),
                        modifier = Modifier.testTag("organise-to-move"),
                    )
                }
                item(key = "in-place") {
                    val unchanged = current.unchanged.toInt()
                    SettingRow(
                        stringResource(R.string.organise_in_place),
                        supporting =
                            pluralStringResource(R.plurals.count_songs, unchanged, unchanged),
                    )
                }
                if (current.taken > 0u) {
                    item(key = "taken") {
                        val taken = current.taken.toInt()
                        SettingRow(
                            stringResource(R.string.organise_taken),
                            supporting = pluralStringResource(R.plurals.count_songs, taken, taken),
                        )
                    }
                }
                if (current.unplaced > 0u) {
                    item(key = "unplaced") {
                        val unplaced = current.unplaced.toInt()
                        SettingRow(
                            stringResource(R.string.organise_unplaced),
                            supporting =
                                pluralStringResource(R.plurals.count_songs, unplaced, unplaced),
                        )
                    }
                }
                item(key = "kept") { Footnote(stringResource(R.string.organise_kept_in_place)) }
                if (moves.isNotEmpty()) {
                    item(key = "moves") { SectionHeader(stringResource(R.string.organise_moves)) }
                    items(moves.take(OrganiseSentences.LISTED_AT_MOST), key = { "move-${it.id}" }) {
                        move ->
                        SettingRow(
                            move.to.joinToString(" / "),
                            supporting = OrganiseSentences.shown(words, move.from, model.rootId),
                        )
                    }
                    if (moves.size > OrganiseSentences.LISTED_AT_MOST) {
                        item(key = "more") {
                            val more = moves.size - OrganiseSentences.LISTED_AT_MOST
                            Note(pluralStringResource(R.plurals.organise_more, more, more))
                        }
                    }
                }
            }
        }
    }
    if (confirming) {
        ConfirmDialog(
            title = OrganiseSentences.confirmTitle(words, moves.size),
            text = OrganiseWords.explained(words, model.name),
            confirm = stringResource(R.string.organise_action),
            icon = Icons.Rounded.DriveFileRenameOutline,
            onConfirm = {
                model.organise(moves)
                navigator.back()
            },
            onDismiss = { confirming = false },
        )
    }
}
