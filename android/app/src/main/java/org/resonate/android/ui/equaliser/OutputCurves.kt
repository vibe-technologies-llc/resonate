package org.resonate.android.ui.equaliser

import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.DeleteOutline
import androidx.compose.material.icons.rounded.SpeakerGroup
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.lifecycle.ViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import kotlinx.coroutines.flow.StateFlow
import org.resonate.android.OutputRoute
import org.resonate.android.R
import org.resonate.android.Words
import org.resonate.android.ui.Resonate
import org.resonate.android.ui.components.EmptyState
import org.resonate.android.ui.components.Page
import org.resonate.android.ui.resonateViewModel
import org.resonate.android.ui.settings.ConfirmDialog
import org.resonate.android.ui.settings.Note
import org.resonate.android.ui.settings.SettingRow
import org.resonate.android.ui.settings.SettingsList

fun isBound(route: OutputRoute?, bound: List<OutputRoute>): Boolean =
    route != null && bound.any { it.id == route.id }

fun keptForOthers(route: OutputRoute?, bound: List<OutputRoute>): List<OutputRoute> =
    bound.filter { it.id != route?.id }

fun curveScope(words: Words, route: OutputRoute?, bound: List<OutputRoute>): String =
    if (route != null && isBound(route, bound)) route.name
    else words.string(R.string.equaliser_every_output)

class OutputCurvesViewModel(resonate: Resonate) : ViewModel() {
    private val library = resonate.library

    val route: StateFlow<OutputRoute?> = library.route

    val bound: StateFlow<List<OutputRoute>> = library.boundRoutes

    fun forget(route: OutputRoute) = library.forgetEqualiser(route)
}

@Composable
fun OutputCurvesScreen() {
    val model = resonateViewModel { OutputCurvesViewModel(it) }
    val route by model.route.collectAsStateWithLifecycle()
    val bound by model.bound.collectAsStateWithLifecycle()
    var forgetting by rememberSaveable { mutableStateOf<String?>(null) }
    val others = keptForOthers(route, bound)
    val asked = others.firstOrNull { it.id == forgetting }
    Page(stringResource(R.string.equaliser_other_curves)) { padding ->
        SettingsList(padding, Modifier.testTag("output-curves-list")) {
            item(key = "note") { Note(stringResource(R.string.equaliser_other_curves_note)) }
            if (others.isEmpty()) {
                item(key = "none") {
                    EmptyState(
                        Icons.Rounded.SpeakerGroup,
                        stringResource(R.string.equaliser_other_curves_none),
                        detail = stringResource(R.string.equaliser_other_curves_none_detail),
                    )
                }
            }
            items(others, key = { it.id }) { other ->
                SettingRow(
                    title = other.name,
                    icon = Icons.Rounded.SpeakerGroup,
                    trailing = {
                        IconButton(onClick = { forgetting = other.id }) {
                            Icon(
                                Icons.Rounded.DeleteOutline,
                                stringResource(R.string.equaliser_forget_curve_for, other.name),
                            )
                        }
                    },
                    modifier = Modifier.testTag("kept-curve"),
                )
            }
        }
    }
    if (asked != null) {
        ConfirmDialog(
            title = stringResource(R.string.equaliser_forget_curve_title, asked.name),
            text = stringResource(R.string.equaliser_forget_curve_text),
            confirm = stringResource(R.string.equaliser_forget),
            onConfirm = { model.forget(asked) },
            onDismiss = { forgetting = null },
            icon = Icons.Rounded.DeleteOutline,
            destructive = true,
        )
    }
}
