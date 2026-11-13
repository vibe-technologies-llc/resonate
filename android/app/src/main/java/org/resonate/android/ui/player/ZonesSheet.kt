package org.resonate.android.ui.player

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Add
import androidx.compose.material.icons.rounded.MoreVert
import androidx.compose.material.icons.rounded.Tune
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import kotlinx.coroutines.delay
import org.resonate.android.R
import org.resonate.android.RendererHeard
import org.resonate.android.ZonePlan
import org.resonate.android.nativecore.NetworkRenderer
import org.resonate.android.nativecore.PlayerZone
import org.resonate.android.nativecore.RendererLearnt
import org.resonate.android.ui.LocalResonate
import org.resonate.android.ui.settings.ConfirmDialog
import org.resonate.android.ui.settings.offeredOutputs
import org.resonate.android.ui.settings.outputNamed
import org.resonate.android.ui.settings.outputTitle
import org.resonate.android.words

@Composable
fun ZonesSheet(onDismiss: () -> Unit) {
    val features = LocalResonate.current.features
    val queue by LocalResonate.current.library.queue.collectAsStateWithLifecycle()
    val zones by features.zones.listed.collectAsStateWithLifecycle()
    val outputs by features.outputs.collectAsStateWithLifecycle()
    var renaming by remember { mutableStateOf<PlayerZone?>(null) }
    var removing by remember { mutableStateOf<PlayerZone?>(null) }
    var choosing by remember { mutableStateOf<PlayerZone?>(null) }
    val looking by features.renderers.looking.collectAsStateWithLifecycle()
    val renderers by features.renderers.listed.collectAsStateWithLifecycle()
    val addresses by features.renderers.addresses.collectAsStateWithLifecycle()
    val receiverAddresses by features.renderers.receiverAddresses.collectAsStateWithLifecycle()
    val learnt by features.renderers.learnt.collectAsStateWithLifecycle()
    val trying by features.renderers.trying.collectAsStateWithLifecycle()
    val heard by features.renderers.heard.collectAsStateWithLifecycle()
    var addressing by remember { mutableStateOf(false) }
    var addressingReceiver by remember { mutableStateOf(false) }
    var learning by remember { mutableStateOf(false) }
    val shown = zones.firstOrNull { it.shown }?.id
    LaunchedEffect(Unit) { features.refreshOutputs() }
    LaunchedEffect(looking, addresses.isNotEmpty()) {
        while (looking || addresses.isNotEmpty()) {
            features.renderers.refresh()
            delay(RENDERERS_READ_EVERY_MS)
        }
    }
    ModalBottomSheet(onDismissRequest = onDismiss) {
        Column(
            Modifier.navigationBarsPadding().padding(bottom = 16.dp).verticalScroll(rememberScrollState())
        ) {
            Text(
                stringResource(R.string.player_zones),
                style = MaterialTheme.typography.titleLarge,
                modifier = Modifier.padding(horizontal = 24.dp),
            )
            Text(
                stringResource(R.string.zones_detail),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(horizontal = 24.dp, vertical = 4.dp),
            )
            Column(Modifier.selectableGroup()) {
                zones.forEach { zone ->
                    ZoneRow(
                        zone = zone,
                        output =
                            ZonePlan.heardOn(
                                zone.rendererName,
                                outputNamed(words(), outputs, zone.device),
                            ),
                        mayHandOver =
                            shown != null && ZonePlan.mayHandOverTo(zone.id, shown, queue.isNotEmpty()),
                        onShow = { features.zones.show(zone.id) },
                        onHandOver = { features.zones.handOverTo(zone.id) },
                        onRename = { renaming = zone },
                        onRemove = { removing = zone },
                        onChooseRenderer = { choosing = zone },
                        onPlayOnPhone = { features.zones.playOn(zone.id, null) },
                    )
                }
            }
            Spacer(Modifier.height(8.dp))
            Text(
                stringResource(R.string.zones_add_heading),
                style = MaterialTheme.typography.titleSmall,
                modifier = Modifier.padding(horizontal = 24.dp, vertical = 8.dp),
            )
            offeredOutputs(outputs).forEach { output ->
                val title = outputTitle(words(), output)
                ListItem(
                    onClick = { features.zones.add(title, output) },
                    colors = ListItemDefaults.colors(containerColor = Color.Transparent),
                    leadingContent = { Icon(Icons.Rounded.Add, contentDescription = null) },
                ) {
                    Text(stringResource(R.string.zones_add_on, title))
                }
            }
            Spacer(Modifier.height(8.dp))
            Text(
                stringResource(R.string.zones_renderers_heading),
                style = MaterialTheme.typography.titleSmall,
                modifier = Modifier.padding(horizontal = 24.dp, vertical = 8.dp),
            )
            ListItem(
                onClick = { features.renderers.look(!looking) },
                colors = ListItemDefaults.colors(containerColor = Color.Transparent),
                supportingContent = {
                    Text(
                        if (looking)
                            pluralStringResource(
                                R.plurals.zones_renderers_found,
                                renderers.size,
                                renderers.size,
                            )
                        else stringResource(R.string.zones_renderers_detail)
                    )
                },
                trailingContent = { Switch(checked = looking, onCheckedChange = null) },
            ) {
                Text(stringResource(R.string.zones_find_renderers))
            }
            ListItem(
                onClick = { addressing = true },
                colors = ListItemDefaults.colors(containerColor = Color.Transparent),
                leadingContent = { Icon(Icons.Rounded.Add, contentDescription = null) },
                supportingContent = {
                    Text(
                        if (addresses.isEmpty())
                            stringResource(R.string.zones_renderer_address_detail)
                        else
                            pluralStringResource(
                                R.plurals.zones_added,
                                addresses.size,
                                addresses.size,
                            )
                    )
                },
            ) {
                Text(stringResource(R.string.zones_add_renderer_by_address))
            }
            ListItem(
                onClick = { addressingReceiver = true },
                colors = ListItemDefaults.colors(containerColor = Color.Transparent),
                leadingContent = { Icon(Icons.Rounded.Add, contentDescription = null) },
                supportingContent = {
                    Text(
                        if (receiverAddresses.isEmpty())
                            stringResource(R.string.zones_receiver_address_detail)
                        else
                            pluralStringResource(
                                R.plurals.zones_added,
                                receiverAddresses.size,
                                receiverAddresses.size,
                            )
                    )
                },
            ) {
                Text(stringResource(R.string.zones_add_receiver_by_address))
            }
            ListItem(
                onClick = { learning = true },
                colors = ListItemDefaults.colors(containerColor = Color.Transparent),
                leadingContent = { Icon(Icons.Rounded.Tune, contentDescription = null) },
                supportingContent = { Text(stringResource(R.string.zones_what_renderers_play_detail)) },
            ) {
                Text(stringResource(R.string.zones_what_renderers_play))
            }
        }
    }
    if (learning) {
        RendererFormatsDialog(
            renderers = renderers,
            learnt = learnt,
            trying = trying,
            heard = heard,
            learn = { features.renderers.learn(it) },
            forget = { features.renderers.forgetWhatItPlays(it) },
            dismiss = { learning = false },
        )
    }
    if (addressing) {
        RendererAddressDialog(
            title = stringResource(R.string.zones_add_renderer_by_address),
            label = stringResource(R.string.zones_description_address),
            placeholder = "http://192.168.1.20:49152/description.xml",
            addresses = addresses,
            add = { features.renderers.add(it) },
            forget = { features.renderers.forget(it) },
            dismiss = { addressing = false },
        )
    }
    if (addressingReceiver) {
        RendererAddressDialog(
            title = stringResource(R.string.zones_add_receiver_by_address),
            label = stringResource(R.string.zones_receiver_address),
            placeholder = stringResource(R.string.zones_receiver_address_example),
            addresses = receiverAddresses,
            add = { features.renderers.addReceiver(it) },
            forget = { features.renderers.forgetReceiver(it) },
            dismiss = { addressingReceiver = false },
        )
    }
    choosing?.let { zone ->
        RendererDialog(
            zone = zone,
            renderers = renderers,
            reaching = looking || addresses.isNotEmpty() || receiverAddresses.isNotEmpty(),
            choose = { features.zones.playOn(zone.id, it.node) },
            dismiss = { choosing = null },
        )
    }
    renaming?.let { zone ->
        RenameZoneDialog(zone, save = { features.zones.rename(zone.id, it) }, dismiss = { renaming = null })
    }
    removing?.let { zone ->
        ConfirmDialog(
            title = stringResource(R.string.zones_remove_title, zone.name),
            text = stringResource(R.string.zones_remove_detail),
            confirm = stringResource(R.string.zones_remove),
            onConfirm = { features.zones.remove(zone.id) },
            onDismiss = { removing = null },
            destructive = true,
        )
    }
}

@Composable
private fun ZoneRow(
    zone: PlayerZone,
    output: String,
    mayHandOver: Boolean,
    onShow: () -> Unit,
    onHandOver: () -> Unit,
    onRename: () -> Unit,
    onRemove: () -> Unit,
    onChooseRenderer: () -> Unit,
    onPlayOnPhone: () -> Unit,
) {
    var menu by remember { mutableStateOf(false) }
    ListItem(
        selected = zone.shown,
        onClick = onShow,
        colors = ListItemDefaults.colors(containerColor = Color.Transparent),
        leadingContent = { RadioButton(selected = zone.shown, onClick = null) },
        supportingContent = {
            Text(ZonePlan.line(words(), output, zone.transport, zone.playing))
        },
        trailingContent = {
            Box {
                IconButton(onClick = { menu = true }) {
                    Icon(
                        Icons.Rounded.MoreVert,
                        contentDescription = stringResource(R.string.zones_options, zone.name),
                    )
                }
                DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                    if (mayHandOver) {
                        DropdownMenuItem(
                            text = { Text(stringResource(R.string.zones_hand_over)) },
                            onClick = {
                                menu = false
                                onHandOver()
                            },
                        )
                    }
                    DropdownMenuItem(
                        text = { Text(stringResource(R.string.zones_play_on_renderer)) },
                        onClick = {
                            menu = false
                            onChooseRenderer()
                        },
                    )
                    if (zone.renderer != null) {
                        DropdownMenuItem(
                            text = { Text(stringResource(R.string.zones_play_on_phone)) },
                            onClick = {
                                menu = false
                                onPlayOnPhone()
                            },
                        )
                    }
                    DropdownMenuItem(
                        text = { Text(stringResource(R.string.zones_rename)) },
                        onClick = {
                            menu = false
                            onRename()
                        },
                    )
                    if (ZonePlan.mayRemove(zone.id)) {
                        DropdownMenuItem(
                            text = { Text(stringResource(R.string.zones_remove)) },
                            onClick = {
                                menu = false
                                onRemove()
                            },
                        )
                    }
                }
            }
        },
    ) {
        Text(zone.name)
    }
}

@Composable
private fun RenameZoneDialog(zone: PlayerZone, save: (String) -> Unit, dismiss: () -> Unit) {
    var name by rememberSaveable { mutableStateOf(zone.name) }
    val focus = remember { FocusRequester() }
    val saved = {
        val chosen = name.trim()
        if (chosen.isNotEmpty()) {
            save(chosen)
            dismiss()
        }
    }
    LaunchedEffect(Unit) { focus.requestFocus() }
    AlertDialog(
        onDismissRequest = dismiss,
        title = { Text(stringResource(R.string.zones_rename_title)) },
        text = {
            OutlinedTextField(
                value = name,
                onValueChange = { name = it.take(ZONE_NAME_AT_MOST) },
                label = { Text(stringResource(R.string.zones_name)) },
                singleLine = true,
                keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done),
                keyboardActions = KeyboardActions(onDone = { saved() }),
                modifier = Modifier.focusRequester(focus),
            )
        },
        confirmButton = {
            TextButton(onClick = saved, enabled = name.isNotBlank()) {
                Text(stringResource(R.string.zones_save))
            }
        },
        dismissButton = {
            TextButton(onClick = dismiss) { Text(stringResource(R.string.zones_cancel)) }
        },
    )
}

@Composable
private fun RendererDialog(
    zone: PlayerZone,
    renderers: List<NetworkRenderer>,
    reaching: Boolean,
    choose: (NetworkRenderer) -> Unit,
    dismiss: () -> Unit,
) {
    AlertDialog(
        onDismissRequest = dismiss,
        title = { Text(stringResource(R.string.zones_play_on_title, zone.name)) },
        text = {
            when {
                !reaching && renderers.isEmpty() ->
                    Text(stringResource(R.string.zones_renderers_unreached))
                renderers.isEmpty() -> Text(stringResource(R.string.zones_renderers_unanswered))
                else ->
                    Column(Modifier.selectableGroup()) {
                        renderers.forEach { renderer ->
                            ListItem(
                                selected = renderer.node == zone.renderer,
                                onClick = {
                                    choose(renderer)
                                    dismiss()
                                },
                                colors = ListItemDefaults.colors(containerColor = Color.Transparent),
                                leadingContent = {
                                    RadioButton(selected = renderer.node == zone.renderer, onClick = null)
                                },
                                supportingContent = {
                                    Text(
                                        ZonePlan.rendererLine(
                                            words(),
                                            renderer.kind,
                                            renderer.host,
                                        )
                                    )

                                },
                            ) {
                                Text(renderer.name)
                            }
                        }
                    }
            }
        },
        confirmButton = {
            TextButton(onClick = dismiss) { Text(stringResource(R.string.zones_close)) }
        },
    )
}

@Composable
private fun RendererFormatsDialog(
    renderers: List<NetworkRenderer>,
    learnt: List<RendererLearnt>,
    trying: String?,
    heard: RendererHeard?,
    learn: (String) -> Unit,
    forget: (String) -> Unit,
    dismiss: () -> Unit,
) {
    val listed = renderers.map { it.node to it.name }
    val away = learnt.filter { kept -> renderers.none { it.node == kept.node } }.map { it.node to it.name }
    AlertDialog(
        onDismissRequest = dismiss,
        title = { Text(stringResource(R.string.zones_what_renderers_play)) },
        text = {
            Column(Modifier.verticalScroll(rememberScrollState())) {
                if (listed.isEmpty() && away.isEmpty()) {
                    Text(stringResource(R.string.zones_tried_renderers_none))
                }
                (listed + away).forEach { (node, name) ->
                    val kept = learnt.firstOrNull { it.node == node }
                    val reachable = listed.any { it.first == node }
                    ListItem(
                        colors = ListItemDefaults.colors(containerColor = Color.Transparent),
                        supportingContent = {
                            Column {
                                when {
                                    trying == node -> {
                                        Text(stringResource(R.string.zones_trying_what_it_plays))
                                        LinearProgressIndicator(Modifier.padding(top = 4.dp))
                                    }
                                    heard?.node == node ->
                                        heard.trials.forEach { trial -> Text(trial.said) }
                                    kept != null -> Text(kept.said)
                                    else -> Text(stringResource(R.string.zones_not_tried_yet))
                                }
                            }
                        },
                        trailingContent = {
                            Row {
                                if (kept != null) {
                                    TextButton(onClick = { forget(node) }, enabled = trying != node) {
                                        Text(stringResource(R.string.zones_forget_what_it_plays))
                                    }
                                }
                                if (reachable) {
                                    TextButton(onClick = { learn(node) }, enabled = trying == null) {
                                        Text(stringResource(R.string.zones_try_what_it_plays))
                                    }
                                }
                            }
                        },
                    ) {
                        Text(name)
                    }
                }
            }
        },
        confirmButton = {
            TextButton(onClick = dismiss) { Text(stringResource(R.string.zones_close)) }
        },
    )
}

@Composable
private fun RendererAddressDialog(
    title: String,
    label: String,
    placeholder: String,
    addresses: List<String>,
    add: (String) -> Unit,
    forget: (String) -> Unit,
    dismiss: () -> Unit,
) {
    var typed by rememberSaveable { mutableStateOf("") }
    val focus = remember { FocusRequester() }
    val added = {
        val chosen = typed.trim()
        if (chosen.isNotEmpty()) {
            add(chosen)
            typed = ""
        }
    }
    LaunchedEffect(Unit) { focus.requestFocus() }
    AlertDialog(
        onDismissRequest = dismiss,
        title = { Text(title) },
        text = {
            Column(Modifier.verticalScroll(rememberScrollState())) {
                OutlinedTextField(
                    value = typed,
                    onValueChange = { typed = it.take(RENDERER_ADDRESS_AT_MOST) },
                    label = { Text(label) },
                    placeholder = { Text(placeholder) },
                    singleLine = true,
                    keyboardOptions =
                        KeyboardOptions(keyboardType = KeyboardType.Uri, imeAction = ImeAction.Done),
                    keyboardActions = KeyboardActions(onDone = { added() }),
                    modifier = Modifier.focusRequester(focus),
                )
                addresses.forEach { address ->
                    ListItem(
                        colors = ListItemDefaults.colors(containerColor = Color.Transparent),
                        trailingContent = {
                            TextButton(onClick = { forget(address) }) {
                                Text(stringResource(R.string.zones_remove))
                            }
                        },
                    ) {
                        Text(address)
                    }
                }
            }
        },
        confirmButton = {
            TextButton(onClick = added, enabled = typed.isNotBlank()) {
                Text(stringResource(R.string.zones_add))
            }
        },
        dismissButton = {
            TextButton(onClick = dismiss) { Text(stringResource(R.string.zones_close)) }
        },

    )
}

private const val RENDERER_ADDRESS_AT_MOST = 512
private const val ZONE_NAME_AT_MOST = 64
private const val RENDERERS_READ_EVERY_MS = 2_000L
