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
import org.resonate.android.nativecore.FiledSong
import org.resonate.android.nativecore.OrganisePlan
import org.resonate.android.nativecore.defaultOrganiseLayout
import org.resonate.android.nativecore.organiseLayoutProblem
import org.resonate.android.ui.Resonate
import org.resonate.android.ui.components.Page
import org.resonate.android.ui.components.SectionHeader
import org.resonate.android.ui.counted
import org.resonate.android.ui.navigation.LocalNavigator
import org.resonate.android.ui.resonateViewModel

data class Planned(val layout: String, val plan: OrganisePlan)

object OrganiseWords {
    const val FIELDS =
        "Fields: {albumartist} {artist} {album} {title} {year} {disc} {track} {ext}. Slashes make folders; {disc} is written only for albums of more than one disc."
    const val KEPT_IN_PLACE =
        "A song whose place another file holds, or that names nothing to file it by, stays where it is."

    fun explained(folder: String): String =
        "The files move inside $folder, each with its lyrics beside it. Favourites, plays and playlists follow them, and Library tools can put them back."

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
        resonate.features.folders.value.firstOrNull { it.uri == folder }?.name ?: "this folder"
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
    Page(
        "Organise ${model.name}",
        actions = {
            TextButton(
                onClick = { confirming = true },
                enabled = problem == null && !planning && moves.isNotEmpty() && !busy,
                modifier = Modifier.testTag("organise"),
            ) {
                Text("Organise")
            }
        },
    ) { padding ->
        SettingsList(padding, Modifier.testTag("organise-list")) {
            item(key = "layout") {
                Field(
                    value = layout,
                    onValueChange = model::type,
                    label = "Layout",
                    supporting = problem ?: OrganiseWords.FIELDS,
                    error = problem != null,
                    keyboardType = KeyboardType.Uri,
                    modifier = Modifier.testTag("organise-layout"),
                )
            }
            if (layout != usual) {
                item(key = "usual") {
                    ButtonRow {
                        OutlinedButton(onClick = { model.type(usual) }) {
                            Text("Use the usual layout")
                        }
                    }
                }
            }
            if (planning || waiting) {
                item(key = "planning") { ListLoading() }
            } else if (current != null) {
                item(key = "preview") { SectionHeader("Preview") }
                item(key = "to-move") {
                    SettingRow(
                        "To move",
                        supporting = counted(moves.size, "song"),
                        modifier = Modifier.testTag("organise-to-move"),
                    )
                }
                item(key = "in-place") {
                    SettingRow("Already in place", supporting = counted(current.unchanged, "song"))
                }
                if (current.taken > 0u) {
                    item(key = "taken") {
                        SettingRow("Place taken", supporting = counted(current.taken, "song"))
                    }
                }
                if (current.unplaced > 0u) {
                    item(key = "unplaced") {
                        SettingRow("Not enough tags", supporting = counted(current.unplaced, "song"))
                    }
                }
                item(key = "kept") { Footnote(OrganiseWords.KEPT_IN_PLACE) }
                if (moves.isNotEmpty()) {
                    item(key = "moves") { SectionHeader("Moves") }
                    items(moves.take(OrganiseSentences.LISTED_AT_MOST), key = { "move-${it.id}" }) {
                        move ->
                        SettingRow(
                            move.to.joinToString(" / "),
                            supporting = OrganiseSentences.shown(move.from, model.rootId),
                        )
                    }
                    if (moves.size > OrganiseSentences.LISTED_AT_MOST) {
                        item(key = "more") {
                            Note(
                                "and ${counted(moves.size - OrganiseSentences.LISTED_AT_MOST, "more song")}"
                            )
                        }
                    }
                }
            }
        }
    }
    if (confirming) {
        ConfirmDialog(
            title = OrganiseSentences.confirmTitle(moves.size),
            text = OrganiseWords.explained(model.name),
            confirm = "Organise",
            icon = Icons.Rounded.DriveFileRenameOutline,
            onConfirm = {
                model.organise(moves)
                navigator.back()
            },
            onDismiss = { confirming = false },
        )
    }
}
