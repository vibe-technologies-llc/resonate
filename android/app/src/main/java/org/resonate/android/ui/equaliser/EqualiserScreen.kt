package org.resonate.android.ui.equaliser

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.expandVertically
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.shrinkVertically
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Add
import androidx.compose.material.icons.rounded.AutoAwesome
import androidx.compose.material.icons.rounded.Cloud
import androidx.compose.material.icons.rounded.ContentPaste
import androidx.compose.material.icons.rounded.Equalizer
import androidx.compose.material.icons.rounded.ExpandMore
import androidx.compose.material.icons.rounded.GraphicEq
import androidx.compose.material.icons.rounded.Headphones
import androidx.compose.material.icons.rounded.HorizontalRule
import androidx.compose.material.icons.rounded.MicNone
import androidx.compose.material.icons.rounded.MusicNote
import androidx.compose.material.icons.rounded.RestartAlt
import androidx.compose.material.icons.rounded.Speaker
import androidx.compose.material.icons.rounded.SpeakerGroup
import androidx.compose.material.icons.rounded.Tune
import androidx.compose.material.icons.rounded.VerticalAlignTop
import androidx.compose.material.icons.rounded.Waves
import androidx.compose.material.icons.rounded.WbSunny
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.FilterChip
import androidx.compose.material3.FilterChipDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.resonate.android.nativecore.EqualiserCurve
import org.resonate.android.nativecore.EqualiserPreset
import org.resonate.android.ui.components.Page
import org.resonate.android.ui.components.SectionHeader
import org.resonate.android.ui.components.Sizes
import org.resonate.android.ui.navigation.LocalNavigator
import org.resonate.android.ui.navigation.Screen
import org.resonate.android.ui.resonateViewModel

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

@Composable
fun EqualiserScreen() {
    val model = resonateViewModel { EqualiserViewModel(it) }
    val info by model.info.collectAsStateWithLifecycle()
    val preferences by model.preferences.collectAsStateWithLifecycle()
    val shown by model.shown.collectAsStateWithLifecycle()
    Page("Equaliser") { padding ->
        val loaded = info
        val curve = shown
        val enabled = preferences?.equaliserEnabled
        if (loaded == null || curve == null || enabled == null) {
            Box(Modifier.fillMaxSize().padding(padding), contentAlignment = Alignment.Center) {
                LoadingIndicator()
            }
        } else {
            EqualiserContent(model, loaded, curve, enabled, padding)
        }
    }
}

@Composable
private fun EqualiserContent(
    model: EqualiserViewModel,
    info: EqualiserInfo,
    curve: EqualiserCurve,
    enabled: Boolean,
    padding: PaddingValues,
) {
    val navigator = LocalNavigator.current
    val route by model.route.collectAsStateWithLifecycle()
    val bound by model.boundRoutes.collectAsStateWithLifecycle()
    val scope = curveScope(route, bound)
    var advanced by rememberSaveable { mutableStateOf(false) }
    var importing by rememberSaveable { mutableStateOf(false) }
    var selected by rememberSaveable { mutableStateOf<Int?>(null) }
    var resetting by rememberSaveable { mutableStateOf(false) }
    val chosen = selected?.takeIf { it in curve.bands.indices }
    val preset = shownPreset(info.presets, curve)
    Box(Modifier.fillMaxSize(), contentAlignment = Alignment.TopCenter) {
        LazyColumn(
            contentPadding = padding,
            modifier = Modifier.widthIn(max = 840.dp).fillMaxWidth().testTag("equaliser-list"),
        ) {
            item(key = "switch") {
                MasterSwitch(enabled, shownName(curve), scope, model::enable)
            }
            item(key = "output-curve") {
                OutputCurveSection(model)
            }
            item(key = "presets") {
                Presets(info.presets, preset, model::choose)
            }
            item(key = "adjust") {
                SectionHeader("Adjust sound", action = "Reset", onAction = { resetting = true })
                Panel {
                    SliderBank(
                        curve = editable(curve, info.frequencies),
                        bounds = info.bounds,
                        onCommit = { index, gain ->
                            model.commit(editable(curve, info.frequencies).withGain(index, gain, info.bounds))
                        },
                        onEdit = { index ->
                            advanced = true
                            selected = index
                        },
                    )
                }
            }
            item(key = "preamp") {
                Preamp(curve, info.bounds, onCommit = { model.commit(curve.withPreamp(it, info.bounds)) }, onFit = { model.fitPreamp(curve) })
            }
            item(key = "advanced") {
                Expander(
                    icon = Icons.Rounded.Tune,
                    title = "Advanced",
                    detail = "Response graph and parametric bands",
                    open = advanced,
                    onToggle = { advanced = it },
                    tag = "equaliser-advanced",
                )
            }
            if (advanced) {
                advancedItems(model, info, curve, chosen) { selected = it }
            }
            item(key = "import") {
                Expander(
                    icon = Icons.Rounded.ContentPaste,
                    title = "Import a profile",
                    detail = "Paste EqualizerAPO or GraphicEQ text",
                    open = importing,
                    onToggle = { importing = it },
                    tag = "equaliser-import",
                )
                AnimatedVisibility(importing, enter = expandVertically() + fadeIn(), exit = shrinkVertically() + fadeOut()) {
                    ImportProfile { name, text ->
                        model.importProfile(name, text) {
                            selected = null
                            importing = false
                        }
                    }
                }
            }
            item(key = "measurement") {
                MeasurementSection(model)
            }
            item(key = "correction") {
                ListItem(
                    onClick = { navigator.open(Screen.HeadphoneCorrection) },
                    modifier = Modifier.testTag("headphone-correction"),
                    colors = ListItemDefaults.colors(containerColor = Color.Transparent),
                    leadingContent = { Icon(Icons.Rounded.Headphones, null) },
                    supportingContent = { Text("Find an AutoEq profile measured for your headphones") },
                ) { Text("Headphone correction") }
            }
            item(key = "room") {
                RoomCorrectionSection(model)
            }
            item(key = "kept-curves") {
                KeptCurves(model)
            }
        }
    }
    if (resetting) {
        AlertDialog(
            onDismissRequest = { resetting = false },
            icon = { Icon(Icons.Rounded.RestartAlt, null) },
            title = { Text("Reset equaliser?") },
            text = { Text("Restore flat sound and return the preamp to 0 dB. The equaliser stays on or off as it is.") },
            confirmButton = {
                TextButton(onClick = {
                    resetting = false
                    selected = null
                    model.reset()
                }) { Text("Reset") }
            },
            dismissButton = { TextButton(onClick = { resetting = false }) { Text("Cancel") } },
        )
    }
}

private fun LazyListScope.advancedItems(
    model: EqualiserViewModel,
    info: EqualiserInfo,
    curve: EqualiserCurve,
    chosen: Int?,
    select: (Int?) -> Unit,
) {
    item(key = "graph") {
        val response by model.response.collectAsStateWithLifecycle()
        Panel {
            Column(Modifier.padding(12.dp)) {
                EqualiserGraph(
                    curve = curve,
                    response = response,
                    bounds = info.bounds,
                    selected = chosen,
                    onSelect = select,
                    onEdit = model::hold,
                    onRelease = model::release,
                    onCommit = { model.commit(it) },
                    onAdd = { hertz, gain ->
                        curve.withAddedBand(hertz, gain, info.bounds)?.let {
                            model.commit(it)
                            select(it.bands.lastIndex)
                        }
                    },
                )
                Text(
                    "Drag a point to move a band. Tap empty space to add one.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(top = 8.dp, start = 4.dp),
                )
            }
        }
    }
    item(key = "bands") {
        FlowRow(
            Modifier.fillMaxWidth().padding(horizontal = Sizes.gutter, vertical = 12.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            curve.bands.forEachIndexed { index, band ->
                FilterChip(
                    selected = chosen == index,
                    onClick = { select(if (chosen == index) null else index) },
                    label = { Text("${index + 1} · ${hertzLabel(band.frequencyHz)}") },
                    modifier = Modifier.testTag("band-chip-$index"),
                )
            }
            FilledTonalButton(
                onClick = {
                    curve.withAddedBand(1000f, 0f, info.bounds)?.let {
                        model.commit(it)
                        select(it.bands.lastIndex)
                    }
                },
                enabled = curve.bands.size < info.bounds.bandsAtMost,
                contentPadding = ButtonDefaults.ButtonWithIconContentPadding,
            ) {
                Icon(Icons.Rounded.Add, null, Modifier.size(ButtonDefaults.IconSize))
                Spacer(Modifier.width(ButtonDefaults.IconSpacing))
                Text("Add a band")
            }
        }
    }
    if (chosen != null) {
        item(key = "editor") {
            BandEditor(
                band = curve.bands[chosen],
                index = chosen,
                bounds = info.bounds,
                onChange = { model.commit(curve.withBand(chosen, it)) },
                onDelete = {
                    select(null)
                    model.commit(curve.withoutBand(chosen))
                },
                modifier = Modifier.padding(horizontal = Sizes.gutter),
            )
        }
    }
}

@Composable
private fun MasterSwitch(enabled: Boolean, name: String, scope: String, onChange: (Boolean) -> Unit) {
    val container by
        animateColorAsState(
            if (enabled) MaterialTheme.colorScheme.primaryContainer else MaterialTheme.colorScheme.surfaceContainerHigh,
            label = "equaliser switch",
        )
    Surface(
        checked = enabled,
        onCheckedChange = onChange,
        color = container,
        shape = MaterialTheme.shapes.extraLarge,
        modifier =
            Modifier.fillMaxWidth()
                .padding(horizontal = Sizes.gutter, vertical = 8.dp)
                .semantics { role = Role.Switch }
                .testTag("equaliser-switch"),
    ) {
        ListItem(
            colors = ListItemDefaults.colors(containerColor = Color.Transparent),
            modifier = Modifier.padding(vertical = 8.dp),
            leadingContent = { Icon(Icons.Rounded.Equalizer, null) },
            supportingContent = {
                Text(
                    if (enabled) "$name · $scope" else "Off · Original sound",
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                )
            },
            trailingContent = { Switch(checked = enabled, onCheckedChange = null) },
        ) { Text("Use equaliser", style = MaterialTheme.typography.titleLarge) }
    }
}

@Composable
private fun Presets(presets: List<EqualiserPreset>, shown: EqualiserPreset?, onChoose: (EqualiserPreset) -> Unit) {
    SectionHeader("Presets")
    FlowRow(
        Modifier.fillMaxWidth().padding(horizontal = Sizes.gutter),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        presets.forEach { preset ->
            val active = preset.name == shown?.name
            FilterChip(
                selected = active,
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
    Text(
        shown?.detail ?: "Your own sound. Adjust the bands to make it yours.",
        style = MaterialTheme.typography.bodyMedium,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = Modifier.padding(horizontal = Sizes.gutter, vertical = 8.dp),
    )
}

@Composable
private fun Preamp(curve: EqualiserCurve, bounds: EqualiserBounds, onCommit: (Float) -> Unit, onFit: () -> Unit) {
    var live by rememberSaveable { mutableStateOf<Float?>(null) }
    SectionHeader("Preamp")
    Panel {
        Column(Modifier.padding(start = 16.dp, end = 16.dp, top = 16.dp, bottom = 8.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(
                    "Overall level before the bands",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.weight(1f),
                )
                Text(
                    signedDb(live ?: curve.preampDb),
                    style = MaterialTheme.typography.titleMedium.copy(fontFeatureSettings = "tnum"),
                    color = MaterialTheme.colorScheme.primary,
                )
            }
            CommitSlider(
                value = curve.preampDb.coerceIn(bounds.preamp),
                range = bounds.preamp,
                description = "Preamp",
                stateText = ::signedDb,
                snap = ::tidyDb,
                look = SliderLook.CENTRED,
                onDraft = { live = it },
                onCommit = onCommit,
                modifier = Modifier.testTag("preamp"),
            )
            FilledTonalButton(
                onClick = onFit,
                enabled = curve.bands.isNotEmpty(),
                contentPadding = ButtonDefaults.ButtonWithIconContentPadding,
                modifier = Modifier.testTag("fit-preamp"),
            ) {
                Icon(Icons.Rounded.VerticalAlignTop, null, Modifier.size(ButtonDefaults.IconSize))
                Spacer(Modifier.width(ButtonDefaults.IconSpacing))
                Text("Fit the preamp")
            }
            Text(
                "Sets the preamp from the curve's own peak, then from what the songs in the queue peak at through it, so the loudest of them reaches full scale and no further.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(top = 8.dp, bottom = 8.dp),
            )
        }
    }
}

@Composable
private fun Expander(
    icon: ImageVector,
    title: String,
    detail: String,
    open: Boolean,
    onToggle: (Boolean) -> Unit,
    tag: String,
) {
    val turn by animateFloatAsState(if (open) 180f else 0f, label = "expander")
    ListItem(
        onClick = { onToggle(!open) },
        modifier = Modifier.testTag(tag),
        colors = ListItemDefaults.colors(containerColor = Color.Transparent),
        leadingContent = { Icon(icon, null) },
        supportingContent = { Text(detail) },
        trailingContent = {
            Icon(Icons.Rounded.ExpandMore, contentDescription = if (open) "Collapse" else "Expand", modifier = Modifier.rotate(turn))
        },
    ) { Text(title) }
}

@Composable
private fun ImportProfile(onApply: (name: String, text: String) -> Unit) {
    var name by rememberSaveable { mutableStateOf("") }
    var text by rememberSaveable { mutableStateOf("") }
    Column(Modifier.fillMaxWidth().padding(horizontal = Sizes.gutter, vertical = 8.dp)) {
        OutlinedTextField(
            value = name,
            onValueChange = { name = it },
            label = { Text("Profile name") },
            singleLine = true,
            modifier = Modifier.fillMaxWidth(),
        )
        Spacer(Modifier.height(12.dp))
        OutlinedTextField(
            value = text,
            onValueChange = { text = it },
            label = { Text("Profile text") },
            placeholder = { Text("Preamp: -6 dB\nFilter 1: ON PK Fc 100 Hz Gain 3 dB Q 0.7") },
            minLines = 4,
            maxLines = 10,
            modifier = Modifier.fillMaxWidth(),
        )
        Text(
            "Parametric filters load as bands; a GraphicEQ line is fitted onto bands.",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(top = 8.dp, start = 4.dp),
        )
        Button(
            onClick = { onApply(name, text) },
            enabled = text.isNotBlank(),
            modifier = Modifier.align(Alignment.End).padding(top = 12.dp),
        ) { Text("Apply profile") }
    }
}

@Composable
fun Panel(modifier: Modifier = Modifier, content: @Composable () -> Unit) {
    Card(
        modifier = modifier.fillMaxWidth().padding(horizontal = Sizes.gutter, vertical = 4.dp),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainer),
        shape = MaterialTheme.shapes.extraLarge,
    ) {
        content()
    }
}
