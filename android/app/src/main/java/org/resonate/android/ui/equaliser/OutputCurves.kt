package org.resonate.android.ui.equaliser

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.DeleteOutline
import androidx.compose.material.icons.rounded.Speaker
import androidx.compose.material.icons.rounded.SpeakerGroup
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
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
import org.resonate.android.OutputRoute
import org.resonate.android.ui.components.SectionHeader
import org.resonate.android.ui.components.Sizes

fun isBound(route: OutputRoute?, bound: List<OutputRoute>): Boolean =
    route != null && bound.any { it.id == route.id }

fun keptForOthers(route: OutputRoute?, bound: List<OutputRoute>): List<OutputRoute> =
    bound.filter { it.id != route?.id }

fun curveScope(route: OutputRoute?, bound: List<OutputRoute>): String =
    if (route != null && isBound(route, bound)) route.name else "Every output"

@Composable
fun OutputCurveSection(model: EqualiserViewModel) {
    val route by model.route.collectAsStateWithLifecycle()
    val bound by model.boundRoutes.collectAsStateWithLifecycle()
    val playing = route ?: return
    val kept = isBound(playing, bound)
    Column {
        Panel {
            ListItem(
                checked = kept,
                onCheckedChange = model::bind,
                modifier = Modifier.testTag("output-curve-switch"),
                colors = ListItemDefaults.colors(containerColor = Color.Transparent),
                leadingContent = { Icon(Icons.Rounded.Speaker, null) },
                trailingContent = { Switch(checked = kept, onCheckedChange = null) },
            ) { Text("Only for ${playing.name}") }
        }
        Text(
            if (kept)
                "This curve is kept for this output and comes back whenever it plays. Other outputs use their own curve or the shared one."
            else "This curve is shared by every output without a curve of its own.",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(horizontal = Sizes.gutter, vertical = 4.dp),
        )
    }
}

@Composable
fun KeptCurves(model: EqualiserViewModel) {
    val route by model.route.collectAsStateWithLifecycle()
    val bound by model.boundRoutes.collectAsStateWithLifecycle()
    var forgetting by rememberSaveable { mutableStateOf<String?>(null) }
    val others = keptForOthers(route, bound)
    if (others.isEmpty()) return
    val asked = others.firstOrNull { it.id == forgetting }
    Column {
        SectionHeader("Curves kept for other outputs")
        Panel {
            Column {
                others.forEach { other ->
                    ListItem(
                        modifier = Modifier.testTag("kept-curve"),
                        colors = ListItemDefaults.colors(containerColor = Color.Transparent),
                        leadingContent = { Icon(Icons.Rounded.SpeakerGroup, null) },
                        trailingContent = {
                            IconButton(onClick = { forgetting = other.id }) {
                                Icon(Icons.Rounded.DeleteOutline, "Forget the curve for ${other.name}")
                            }
                        },
                    ) { Text(other.name) }
                }
            }
        }
        Text(
            "Each comes back when that output plays. Forgetting one lets it use the shared curve.",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(horizontal = Sizes.gutter, vertical = 4.dp),
        )
    }
    if (asked != null) {
        AlertDialog(
            onDismissRequest = { forgetting = null },
            icon = { Icon(Icons.Rounded.DeleteOutline, null) },
            title = { Text("Forget the curve for ${asked.name}?") },
            text = { Text("That output will use the shared curve. You can bind a curve to it again later.") },
            confirmButton = {
                TextButton(
                    onClick = {
                        forgetting = null
                        model.forget(asked)
                    },
                    colors = ButtonDefaults.textButtonColors(contentColor = MaterialTheme.colorScheme.error),
                ) { Text("Forget") }
            },
            dismissButton = { TextButton(onClick = { forgetting = null }) { Text("Cancel") } },
        )
    }
}
