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
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Flare
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
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.resonate.android.OutputRoute
import org.resonate.android.R
import org.resonate.android.RoomView
import org.resonate.android.Words
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
import org.resonate.android.words

private val presetIcons: List<ImageVector> =
    listOf(
        Icons.Rounded.HorizontalRule,
        Icons.Rounded.Speaker,
        Icons.Rounded.SpeakerGroup,
        Icons.Rounded.Waves,
        Icons.Rounded.MicNone,
        Icons.Rounded.WbSunny,
        Icons.Rounded.Flare,
        Icons.Rounded.Cloud,
        Icons.Rounded.MusicNote,
        Icons.Rounded.GraphicEq,
    )

fun roomStatus(words: Words, room: RoomView): String =
    when (val name = room.name) {
        null -> words.string(R.string.equaliser_room_none)
        else ->
            words.string(
                if (room.on) R.string.equaliser_room_on else R.string.equaliser_room_off,
                name,
            )
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
    Page(
        stringResource(R.string.equaliser_title),
        actions = { MoreMenu(onReset = { resetting = true }) },
    ) { padding ->
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
            title = stringResource(R.string.equaliser_reset_title),
            text = stringResource(R.string.equaliser_reset_text),
            confirm = stringResource(R.string.equaliser_reset),
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
            Icon(
                Icons.Rounded.MoreVert,
                contentDescription = stringResource(R.string.equaliser_more_options),
            )
        }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            DropdownMenuItem(
                text = { Text(stringResource(R.string.equaliser_reset_curve)) },
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
    val words = words()
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
                title = stringResource(R.string.equaliser_title),
                checked = enabled,
                onCheckedChange = model::enable,
                icon = Icons.Rounded.Equalizer,
                supporting =
                    if (enabled) {
                        "${shownName(words, curve)} · ${curveScope(words, route, bound)}"
                    } else stringResource(R.string.equaliser_off_original),
                modifier = Modifier.testTag("equaliser-switch"),
            )
        }
        route?.let { playing ->
            item(key = "output-curve") {
                val kept = isBound(playing, bound)
                SwitchRow(
                    title = stringResource(R.string.equaliser_only_for, playing.name),
                    checked = kept,
                    onCheckedChange = model::bind,
                    icon = Icons.Rounded.Speaker,
                    supporting =
                        stringResource(
                            if (kept) R.string.equaliser_kept_alone
                            else R.string.equaliser_shared_by_every
                        ),
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
            SectionHeader(stringResource(R.string.equaliser_preamp))
            Column(Modifier.padding(horizontal = Sizes.gutter)) {
                MeasureControl(
                    measure = preampMeasure(words, bounds),
                    value = curve.preampDb,
                    description = stringResource(R.string.equaliser_preamp),
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
                            Text(stringResource(R.string.equaliser_fit))
                        }
                    },
                )
            }
            Note(stringResource(R.string.equaliser_fit_note))
        }
        item(key = "presets") {
            Presets(info.presets, shownPreset(info.presets, curve), model::choose)
        }
        item(key = "corrections") {
            SectionHeader(stringResource(R.string.equaliser_corrections))
        }
        item(key = "headphones") {
            SettingRow(
                title = stringResource(R.string.equaliser_headphone_correction),
                icon = Icons.Rounded.Headphones,
                supporting = stringResource(R.string.equaliser_headphone_correction_detail),
                onClick = { navigator.open(Screen.HeadphoneCorrection) },
                modifier = Modifier.testTag("headphone-correction"),
            )
        }
        item(key = "measurement") {
            SettingRow(
                title = stringResource(R.string.equaliser_measure),
                icon = Icons.Rounded.Mic,
                supporting = stringResource(R.string.equaliser_measure_detail),
                onClick = { navigator.open(Screen.Measurement) },
                modifier = Modifier.testTag("measurement"),
            )
        }
        item(key = "room") {
            SettingRow(
                title = stringResource(R.string.equaliser_room_correction),
                icon = Icons.Rounded.SurroundSound,
                supporting = roomStatus(words, room),
                onClick = { navigator.open(Screen.RoomCorrection) },
                modifier = Modifier.testTag("room-correction"),
            )
        }
        item(key = "listening-test") {
            SettingRow(
                title = stringResource(R.string.equaliser_listening_test),
                icon = Icons.Rounded.Hearing,
                supporting = stringResource(R.string.equaliser_listening_test_detail),
                onClick = { navigator.open(Screen.ListeningTest) },
                modifier = Modifier.testTag("listening-test"),
            )
        }
        if (others.isNotEmpty()) {
            item(key = "kept-curves") {
                SettingRow(
                    title = stringResource(R.string.equaliser_other_curves),
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
    SectionHeader(stringResource(R.string.equaliser_presets))
    LazyRow(
        state = state,
        contentPadding = PaddingValues(horizontal = Sizes.gutter),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        modifier = Modifier.testTag("presets"),
    ) {
        itemsIndexed(presets, key = { _, preset -> preset.name }) { index, preset ->
            FilterChip(
                selected = preset.name == shown?.name,
                onClick = { onChoose(preset) },
                label = { Text(preset.name) },
                leadingIcon = {
                    Icon(
                        presetIcons.getOrNull(index) ?: Icons.Rounded.Equalizer,
                        null,
                        Modifier.size(FilterChipDefaults.IconSize),
                    )
                },
                modifier = Modifier.testTag("preset-${preset.name}"),
            )
        }
    }
    Note(shown?.detail ?: stringResource(R.string.equaliser_own_sound))
}
