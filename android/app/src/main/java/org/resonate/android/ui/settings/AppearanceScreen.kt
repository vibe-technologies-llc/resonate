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
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.BrightnessAuto
import androidx.compose.material.icons.rounded.Check
import androidx.compose.material.icons.rounded.Contrast
import androidx.compose.material.icons.rounded.DarkMode
import androidx.compose.material.icons.rounded.Gradient
import androidx.compose.material.icons.rounded.InvertColors
import androidx.compose.material.icons.rounded.LightMode
import androidx.compose.material.icons.rounded.Wallpaper
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.TextButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import kotlin.math.roundToInt
import org.resonate.android.Accent
import org.resonate.android.Appearance
import org.resonate.android.R
import org.resonate.android.ThemeMode
import org.resonate.android.words
import org.resonate.android.ui.Resonate
import org.resonate.android.ui.components.CommitSlider
import org.resonate.android.ui.components.Page
import org.resonate.android.ui.components.SectionHeader
import org.resonate.android.ui.components.Sizes
import org.resonate.android.ui.resonateViewModel
import org.resonate.android.ui.theme.LocalDarkTheme
import org.resonate.android.ui.theme.schemeOf
import org.resonate.android.ui.theme.wallpaperSeed

class AppearanceViewModel(resonate: Resonate) : ViewModel() {
    private val store = resonate.features.appearance

    val appearance = store.appearance

    fun chooseMode(mode: ThemeMode) = store.chooseMode(mode)

    fun chooseAccent(accent: Accent?) = store.chooseAccent(accent)

    fun choosePitchBlack(pitchBlack: Boolean) = store.choosePitchBlack(pitchBlack)

    fun chooseHighContrast(highContrast: Boolean) = store.chooseHighContrast(highContrast)

    fun chooseBackgroundStrength(strength: Float) = store.chooseBackgroundStrength(strength)

    fun chooseNowPlayingIntensity(intensity: Float) = store.chooseNowPlayingIntensity(intensity)
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
private val SWATCH_HIGHLIGHT_CORNER = 16.dp
private val SLIDER_TRACK = 4.dp
private const val SWATCHES_A_ROW = 4

@Composable
fun AppearanceScreen() {
    val model = resonateViewModel { AppearanceViewModel(it) }
    val appearance by model.appearance.collectAsStateWithLifecycle()
    Page(stringResource(R.string.settings_appearance_title)) { padding ->
        SettingsList(padding, Modifier.testTag("appearance-list")) {
            item(key = "theme") { SectionHeader(stringResource(R.string.settings_inner_theme)) }
            items(ThemeMode.entries, key = { "theme-${it.name}" }) { mode ->
                RadioRow(
                    title = stringResource(mode.label),
                    selected = appearance.mode == mode,
                    onClick = { model.chooseMode(mode) },
                    icon = mode.icon,
                    supporting = stringResource(mode.detail),
                    modifier = Modifier.testTag("theme-${mode.name.lowercase()}"),
                )
            }
            item(key = "pitch-black") {
                SwitchRow(
                    title = stringResource(R.string.settings_inner_pitch_black),
                    checked = appearance.pitchBlack,
                    onCheckedChange = model::choosePitchBlack,
                    icon = Icons.Rounded.Contrast,
                    supporting = stringResource(R.string.appearance_pitch_black_detail),
                    modifier = Modifier.testTag("pitch-black"),
                )
            }
            item(key = "high-contrast") {
                SwitchRow(
                    title = stringResource(R.string.settings_inner_high_contrast),
                    checked = appearance.highContrast,
                    onCheckedChange = model::chooseHighContrast,
                    icon = Icons.Rounded.InvertColors,
                    supporting = stringResource(R.string.appearance_high_contrast_detail),
                    modifier = Modifier.testTag("high-contrast"),
                )
            }
            item(key = "accent") {
                SectionHeader(stringResource(R.string.settings_inner_accent_colour))
            }
            item(key = "swatches") { Swatches(appearance, model::chooseAccent) }
            item(key = "note") {
                Footnote(stringResource(R.string.appearance_accent_note))
            }
            item(key = "background") {
                SectionHeader(stringResource(R.string.settings_inner_player_background))
            }
            item(key = "strength") {
                BackgroundStrength(appearance.backgroundStrength, model::chooseBackgroundStrength)
            }
            item(key = "strength-note") {
                Footnote(stringResource(R.string.appearance_strength_note))
            }
            item(key = "intensity") {
                NowPlayingIntensity(
                    appearance.nowPlayingIntensity,
                    model::chooseNowPlayingIntensity,
                )
            }
            item(key = "intensity-note") {
                Footnote(stringResource(R.string.appearance_intensity_note))
            }
        }
    }
}

@Composable
private fun PercentSetting(
    title: String,
    icon: ImageVector,
    described: String,
    tag: String,
    stored: Float,
    onCommit: (Float) -> Unit,
) {
    val words = words()
    var draft by remember { mutableStateOf<Float?>(null) }
    val shown = draft ?: stored
    Column(Modifier.fillMaxWidth()) {
        SettingRow(
            title = title,
            icon = icon,
            trailing = {
                Text(
                    "${(shown * 100).roundToInt()} %",
                    style = MaterialTheme.typography.labelLarge,
                    color = MaterialTheme.colorScheme.primary,
                )
            },
        )
        CommitSlider(
            value = stored,
            onCommit = onCommit,
            onDraft = { draft = it },
            trackHeight = SLIDER_TRACK,
            stateText = {
                words.plural(
                    R.plurals.appearance_percent_spoken,
                    (it * 100).roundToInt(),
                    (it * 100).roundToInt(),
                )
            },
            modifier =
                Modifier.fillMaxWidth()
                    .padding(start = 56.dp, end = Sizes.gutter)
                    .semantics { contentDescription = described }
                    .testTag(tag),
        )
    }
}

@Composable
private fun BackgroundStrength(stored: Float, onCommit: (Float) -> Unit) {
    PercentSetting(
        title = stringResource(R.string.appearance_strength),
        icon = Icons.Rounded.Gradient,
        described = stringResource(R.string.appearance_strength_described),
        tag = "background-strength",
        stored = stored,
        onCommit = onCommit,
    )
}

@Composable
private fun NowPlayingIntensity(stored: Float, onCommit: (Float) -> Unit) {
    Column(Modifier.fillMaxWidth()) {
        PercentSetting(
            title = stringResource(R.string.appearance_intensity),
            icon = Icons.Rounded.InvertColors,
            described = stringResource(R.string.settings_inner_player_intensity),
            tag = "now-playing-intensity",
            stored = stored,
            onCommit = onCommit,
        )
        if (stored != Appearance.INTENSITY_DEFAULT) {
            TextButton(
                onClick = { onCommit(Appearance.INTENSITY_DEFAULT) },
                modifier =
                    Modifier.padding(start = 48.dp).testTag("now-playing-intensity-reset"),
            ) {
                Text(stringResource(R.string.appearance_intensity_reset))
            }
        }
    }
}

@Composable
private fun Swatches(appearance: Appearance, onChoose: (Accent?) -> Unit) {
    val context = LocalContext.current
    val dark = LocalDarkTheme.current
    val wallpaper = remember(context, dark) { schemeOf(wallpaperSeed(context), dark).primary }
    FlowRow(
        Modifier.fillMaxWidth().padding(horizontal = Sizes.gutter),
        horizontalArrangement = Arrangement.SpaceBetween,
        maxItemsInEachRow = SWATCHES_A_ROW,
    ) {
        Swatch(
            name = stringResource(Appearance.WALLPAPER),
            color = wallpaper,
            chosen = appearance.accent == null,
            onChoose = { onChoose(null) },
            modifier = Modifier.testTag("accent-wallpaper"),
            icon = Icons.Rounded.Wallpaper,
        )
        Accent.entries.forEach { accent ->
            Swatch(
                name = stringResource(accent.label),
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
    icon: ImageVector? = null,
) {
    val ink = if (color.luminance() > 0.4f) Color.Black else Color.White
    Column(
        modifier
            .width(SWATCH_COLUMN)
            .clip(RoundedCornerShape(SWATCH_HIGHLIGHT_CORNER))
            .selectable(selected = chosen, onClick = onChoose, role = Role.RadioButton)
            .padding(vertical = 8.dp),
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
