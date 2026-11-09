package org.resonate.android.ui.equaliser

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.AutoAwesome
import androidx.compose.material.icons.rounded.Cloud
import androidx.compose.material.icons.rounded.Equalizer
import androidx.compose.material.icons.rounded.GraphicEq
import androidx.compose.material.icons.rounded.Headphones
import androidx.compose.material.icons.rounded.Hearing
import androidx.compose.material.icons.rounded.HorizontalRule
import androidx.compose.material.icons.rounded.Mic
import androidx.compose.material.icons.rounded.MicNone
import androidx.compose.material.icons.rounded.MoreVert
import androidx.compose.material.icons.rounded.MusicNote
import androidx.compose.material.icons.rounded.RestartAlt
import androidx.compose.material.icons.rounded.Speaker
import androidx.compose.material.icons.rounded.SpeakerGroup
import androidx.compose.material.icons.rounded.SurroundSound
import androidx.compose.material.icons.rounded.VerticalAlignTop
import androidx.compose.material.icons.rounded.Waves
import androidx.compose.material.icons.rounded.WbSunny
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.FilterChip
import androidx.compose.material3.FilterChipDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.resonate.android.OutputRoute
import org.resonate.android.RoomView
import org.resonate.android.nativecore.EqualiserCurve
import org.resonate.android.nativecore.EqualiserPreset
import org.resonate.android.ui.components.Page
import org.resonate.android.ui.components.SectionHeader
import org.resonate.android.ui.components.Sizes
import org.resonate.android.ui.navigation.LocalNavigator
import org.resonate.android.ui.navigation.Screen
import org.resonate.android.ui.resonateViewModel
import org.resonate.android.ui.settings.ConfirmDialog
import org.resonate.android.ui.settings.Loading
import org.resonate.android.ui.settings.Note
import org.resonate.android.ui.settings.SettingRow
import org.resonate.android.ui.settings.SettingsList
import org.resonate.android.ui.settings.SwitchRow

private val presetIcons: Map<String, ImageVector> =
    mapOf(
        "Flat" to Icons.Rounded.HorizontalRule,
        "Bass boost" to Icons.Rounded.Speaker,
        "Bass only" to Icons.Rounded.SpeakerGroup,
        "Bass cut" to Icons.Rounded.Waves,
        "Vocals" to Icons.Rounded.MicNone,
        "Warm" to Icons.Rounded.WbSunny,
        "Bright" to Icons.Rounded.AutoAwesome,
        "Treble cut" to Icons.Rounded.Cloud,
        "Rock" to Icons.Rounded.MusicNote,
        "Electronic" to Icons.Rounded.GraphicEq,
    )

const val OWN_SOUND = "Your own sound, shaped on the graph above."

const val FIT_NOTE = "Fit brings the queue's loudest peak to full scale."

fun roomStatus(room: RoomView): String =
    when (val name = room.name) {
        null -> "No response chosen"
        else -> if (room.on) "On · $name" else "Off · $name"
    }

fun othersStatus(others: List<OutputRoute>): String = others.joinToString(" · ") { it.name }

@Composable
fun EqualiserScreen() {
    val model = resonateViewModel { EqualiserViewModel(it) }
    val info by model.info.collectAsStateWithLifecycle()
    val preferences by model.preferences.collectAsStateWithLifecycle()
    val shown by model.shown.collectAsStateWithLifecycle()
    var selected by rememberSaveable { mutableStateOf<Int?>(null) }
    var resetting by rememberSaveable { mutableStateOf(false) }
    Page("Equaliser", actions = { MoreMenu(onReset = { resetting = true }) }) { padding ->
        val loaded = info
        val curve = shown
        val enabled = preferences?.equaliserEnabled
        if (loaded == null || curve == null || enabled == null) {
            Loading(padding)
        } else {
            EqualiserContent(model, loaded, curve, enabled, selected, { selected = it }, padding)
        }
    }
    if (resetting) {
        ConfirmDialog(
            title = "Reset curve?",
            text =
                "Every band goes and the preamp returns to 0 dB. The equaliser stays on or off as it is.",
            confirm = "Reset",
            onConfirm = {
                selected = null
                model.reset()
            },
            onDismiss = { resetting = false },
            icon = Icons.Rounded.RestartAlt,
        )
    }
}

@Composable
private fun MoreMenu(onReset: () -> Unit) {
    var open by remember { mutableStateOf(false) }
    Box {
        IconButton(onClick = { open = true }, modifier = Modifier.testTag("equaliser-more")) {
            Icon(Icons.Rounded.MoreVert, contentDescription = "More options")
        }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            DropdownMenuItem(
                text = { Text("Reset curve") },
                leadingIcon = { Icon(Icons.Rounded.RestartAlt, null) },
                onClick = {
                    open = false
                    onReset()
                },
                modifier = Modifier.testTag("reset-curve"),
            )
        }
    }
}

@Composable
private fun EqualiserContent(
    model: EqualiserViewModel,
    info: EqualiserInfo,
    curve: EqualiserCurve,
    enabled: Boolean,
    selected: Int?,
    select: (Int?) -> Unit,
    padding: PaddingValues,
) {
    val navigator = LocalNavigator.current
    val route by model.route.collectAsStateWithLifecycle()
    val bound by model.boundRoutes.collectAsStateWithLifecycle()
    val room by model.room.collectAsStateWithLifecycle()
    val response by model.response.collectAsStateWithLifecycle()
    val bounds = info.bounds
    val editing = editable(curve, info.frequencies)
    val chosen = chosenBand(selected, editing.bands.size)
    val others = keptForOthers(route, bound)
    SettingsList(padding, Modifier.testTag("equaliser-list")) {
        item(key = "switch") {
            SwitchRow(
                title = "Equaliser",
                checked = enabled,
                onCheckedChange = model::enable,
                icon = Icons.Rounded.Equalizer,
                supporting =
                    if (enabled) "${shownName(curve)} · ${curveScope(route, bound)}"
                    else "Off · Original sound",
                modifier = Modifier.testTag("equaliser-switch"),
            )
        }
        route?.let { playing ->
            item(key = "output-curve") {
                val kept = isBound(playing, bound)
                SwitchRow(
                    title = "Only for ${playing.name}",
                    checked = kept,
                    onCheckedChange = model::bind,
                    icon = Icons.Rounded.Speaker,
                    supporting =
                        if (kept) "Kept for this output alone" else "Shared by every output",
                    modifier = Modifier.testTag("output-curve-switch"),
                )
            }
        }
        item(key = "graph") {
            EqualiserCard(Modifier.padding(top = 8.dp)) {
                EqualiserGraph(
                    curve = editing,
                    response = response,
                    bounds = bounds,
                    selected = chosen,
                    onSelect = select,
                    onEdit = model::hold,
                    onRelease = model::release,
                    onCommit = model::commit,
                )
            }
        }
        item(key = "bands") {
            BandChips(
                curve = editing,
                chosen = chosen,
                bounds = bounds,
                onSelect = select,
                onAdd = {
                    editing.withFreeBand(bounds)?.let {
                        model.commit(it)
                        select(it.bands.lastIndex)
                    }
                },
            )
        }
        if (chosen != null) {
            item(key = "band") {
                BandCard(
                    band = editing.bands[chosen],
                    index = chosen,
                    bounds = bounds,
                    onChange = { model.commit(editing.withBand(chosen, it)) },
                    onRemove = {
                        select(chosenAfterRemoving(chosen, editing.bands.size))
                        model.commit(editing.withoutBand(chosen))
                    },
                )
            }
        }
        item(key = "preamp") {
            SectionHeader("Preamp")
            Column(Modifier.padding(horizontal = Sizes.gutter)) {
                MeasureControl(
                    measure = preampMeasure(bounds),
                    value = curve.preampDb,
                    description = "Preamp",
                    onCommit = { model.commit(curve.withPreamp(it, bounds)) },
                    tag = "preamp",
                    action = {
                        FilledTonalButton(
                            onClick = { model.fitPreamp(curve) },
                            enabled = curve.bands.isNotEmpty(),
                            contentPadding = ButtonDefaults.ButtonWithIconContentPadding,
                            modifier = Modifier.padding(start = 8.dp).testTag("fit-preamp"),
                        ) {
                            Icon(
                                Icons.Rounded.VerticalAlignTop,
                                null,
                                Modifier.size(ButtonDefaults.IconSize),
                            )
                            Spacer(Modifier.width(ButtonDefaults.IconSpacing))
                            Text("Fit")
                        }
                    },
                )
            }
            Note(FIT_NOTE)
        }
        item(key = "presets") {
            Presets(info.presets, shownPreset(info.presets, curve), model::choose)
        }
        item(key = "corrections") { SectionHeader("Corrections") }
        item(key = "headphones") {
            SettingRow(
                title = "Headphone correction",
                icon = Icons.Rounded.Headphones,
                supporting = "Search AutoEq or paste a profile",
                onClick = { navigator.open(Screen.HeadphoneCorrection) },
                modifier = Modifier.testTag("headphone-correction"),
            )
        }
        item(key = "measurement") {
            SettingRow(
                title = "Measure with a microphone",
                icon = Icons.Rounded.Mic,
                supporting = "Fit a measurement or a sweep toward a target",
                onClick = { navigator.open(Screen.Measurement) },
                modifier = Modifier.testTag("measurement"),
            )
        }
        item(key = "room") {
            SettingRow(
                title = "Room correction",
                icon = Icons.Rounded.SurroundSound,
                supporting = roomStatus(room),
                onClick = { navigator.open(Screen.RoomCorrection) },
                modifier = Modifier.testTag("room-correction"),
            )
        }
        item(key = "listening-test") {
            SettingRow(
                title = "Listening test",
                icon = Icons.Rounded.Hearing,
                supporting = "A blind ABX test of the curve or a setting",
                onClick = { navigator.open(Screen.ListeningTest) },
                modifier = Modifier.testTag("listening-test"),
            )
        }
        if (others.isNotEmpty()) {
            item(key = "kept-curves") {
                SettingRow(
                    title = "Curves for other outputs",
                    icon = Icons.Rounded.SpeakerGroup,
                    supporting = othersStatus(others),
                    onClick = { navigator.open(Screen.OutputCurves) },
                    modifier = Modifier.testTag("output-curves"),
                )
            }
        }
    }
}

@Composable
private fun Presets(
    presets: List<EqualiserPreset>,
    shown: EqualiserPreset?,
    onChoose: (EqualiserPreset) -> Unit,
) {
    val first = presets.indexOfFirst { it.name == shown?.name }.coerceAtLeast(0)
    val state = rememberLazyListState(initialFirstVisibleItemIndex = first)
    SectionHeader("Presets")
    LazyRow(
        state = state,
        contentPadding = PaddingValues(horizontal = Sizes.gutter),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        modifier = Modifier.testTag("presets"),
    ) {
        items(presets, key = { it.name }) { preset ->
            FilterChip(
                selected = preset.name == shown?.name,
                onClick = { onChoose(preset) },
                label = { Text(preset.name) },
                leadingIcon = {
                    Icon(
                        presetIcons[preset.name] ?: Icons.Rounded.Equalizer,
                        null,
                        Modifier.size(FilterChipDefaults.IconSize),
                    )
                },
                modifier = Modifier.testTag("preset-${preset.name}"),
            )
        }
    }
    Note(shown?.detail ?: OWN_SOUND)
}
