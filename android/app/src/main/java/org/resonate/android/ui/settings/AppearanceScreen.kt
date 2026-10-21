package org.resonate.android.ui.settings

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.BrightnessAuto
import androidx.compose.material.icons.rounded.Check
import androidx.compose.material.icons.rounded.DarkMode
import androidx.compose.material.icons.rounded.Gradient
import androidx.compose.material.icons.rounded.LightMode
import androidx.compose.material.icons.rounded.Wallpaper
import androidx.compose.material3.Icon
import androidx.compose.material3.Slider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import kotlin.math.roundToInt
import org.resonate.android.Accent
import org.resonate.android.Appearance
import org.resonate.android.ThemeMode
import org.resonate.android.ui.Resonate
import org.resonate.android.ui.components.Page
import org.resonate.android.ui.components.SectionHeader
import org.resonate.android.ui.components.Sizes
import org.resonate.android.ui.resonateViewModel
import org.resonate.android.ui.theme.LocalDarkTheme

class AppearanceViewModel(resonate: Resonate) : ViewModel() {
    private val store = resonate.features.appearance

    val appearance = store.appearance

    fun chooseMode(mode: ThemeMode) = store.chooseMode(mode)

    fun chooseAccent(accent: Accent?) = store.chooseAccent(accent)

    fun chooseBackgroundStrength(strength: Float) = store.chooseBackgroundStrength(strength)
}

private val ThemeMode.icon
    get() =
        when (this) {
            ThemeMode.SYSTEM -> Icons.Rounded.BrightnessAuto
            ThemeMode.LIGHT -> Icons.Rounded.LightMode
            ThemeMode.DARK -> Icons.Rounded.DarkMode
        }

private val SWATCH = 48.dp
private val SWATCH_COLUMN = 72.dp

@Composable
fun AppearanceScreen() {
    val model = resonateViewModel { AppearanceViewModel(it) }
    val appearance by model.appearance.collectAsStateWithLifecycle()
    Page("Appearance") { padding ->
        SettingsList(padding, Modifier.testTag("appearance-list")) {
            item(key = "theme") { SectionHeader("Theme") }
            items(ThemeMode.entries, key = { "theme-${it.name}" }) { mode ->
                RadioRow(
                    title = mode.label,
                    selected = appearance.mode == mode,
                    onClick = { model.chooseMode(mode) },
                    icon = mode.icon,
                    supporting = mode.detail,
                    modifier = Modifier.testTag("theme-${mode.name.lowercase()}"),
                )
            }
            item(key = "accent") { SectionHeader("Accent colour") }
            item(key = "swatches") { Swatches(appearance, model::chooseAccent) }
            item(key = "note") {
                Footnote(
                    "Wallpaper takes its colours from your wallpaper. The other accents are " +
                        "Resonate's own, the ones the desktop wears."
                )
            }
            item(key = "background") { SectionHeader("Player background") }
            item(key = "strength") {
                BackgroundStrength(appearance.backgroundStrength, model::chooseBackgroundStrength)
            }
            item(key = "strength-note") {
                Footnote("How strongly the song's cover colours the background of the player.")
            }
        }
    }
}

@Composable
private fun BackgroundStrength(stored: Float, onCommit: (Float) -> Unit) {
    var draft by remember { mutableStateOf<Float?>(null) }
    val shown = draft ?: stored
    Column(Modifier.fillMaxWidth()) {
        SettingRow(
            title = "Strength",
            icon = Icons.Rounded.Gradient,
            trailing = {
                Text(
                    "${(shown * 100).roundToInt()} %",
                    style = MaterialTheme.typography.labelLarge,
                    color = MaterialTheme.colorScheme.primary,
                )
            },
        )
        Slider(
            value = shown,
            onValueChange = { draft = it },
            onValueChangeFinished = {
                draft?.let(onCommit)
                draft = null
            },
            modifier =
                Modifier.fillMaxWidth()
                    .padding(start = 56.dp, end = Sizes.gutter)
                    .semantics {
                        contentDescription = "Player background strength"
                        stateDescription = "${(shown * 100).roundToInt()} percent"
                    }
                    .testTag("background-strength"),
        )
    }
}

@Composable
private fun Swatches(appearance: Appearance, onChoose: (Accent?) -> Unit) {
    val context = LocalContext.current
    val wallpaper =
        if (LocalDarkTheme.current) dynamicDarkColorScheme(context).primary
        else dynamicLightColorScheme(context).primary
    FlowRow(
        Modifier.padding(horizontal = Sizes.gutter, vertical = 8.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        Swatch(
            name = Appearance.WALLPAPER,
            color = wallpaper,
            chosen = appearance.accent == null,
            onChoose = { onChoose(null) },
            modifier = Modifier.testTag("accent-wallpaper"),
            icon = Icons.Rounded.Wallpaper,
        )
        Accent.entries.forEach { accent ->
            Swatch(
                name = accent.label,
                color = Color(0xFF000000.toInt() or accent.hex),
                chosen = appearance.accent == accent,
                onChoose = { onChoose(accent) },
                modifier = Modifier.testTag("accent-${accent.name.lowercase()}"),
            )
        }
    }
}

@Composable
private fun Swatch(
    name: String,
    color: Color,
    chosen: Boolean,
    onChoose: () -> Unit,
    modifier: Modifier = Modifier,
    icon: androidx.compose.ui.graphics.vector.ImageVector? = null,
) {
    val ink = if (color.luminance() > 0.4f) Color.Black else Color.White
    Column(
        modifier
            .width(SWATCH_COLUMN)
            .selectable(selected = chosen, onClick = onChoose, role = Role.RadioButton),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        Surface(
            shape = CircleShape,
            color = color,
            border =
                if (chosen) BorderStroke(3.dp, MaterialTheme.colorScheme.onSurface) else null,
            modifier = Modifier.size(SWATCH),
        ) {
            val shown = if (chosen) Icons.Rounded.Check else icon
            if (shown != null) {
                Icon(shown, contentDescription = null, tint = ink, modifier = Modifier.padding(12.dp))
            }
        }
        Text(
            name,
            style = MaterialTheme.typography.labelMedium,
            color =
                if (chosen) MaterialTheme.colorScheme.onSurface
                else MaterialTheme.colorScheme.onSurfaceVariant,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            textAlign = TextAlign.Center,
        )
    }
}
