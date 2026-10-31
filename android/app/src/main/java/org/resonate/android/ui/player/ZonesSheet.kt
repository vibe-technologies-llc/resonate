package org.resonate.android.ui.player

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
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
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
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
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import kotlinx.coroutines.delay
import org.resonate.android.ZonePlan
import org.resonate.android.nativecore.NetworkRenderer
import org.resonate.android.nativecore.PlayerZone
import org.resonate.android.ui.LocalResonate
import org.resonate.android.ui.settings.ConfirmDialog
import org.resonate.android.ui.settings.outputNamed
import org.resonate.android.ui.settings.outputTitle
import org.resonate.android.ui.settings.withSystemDefault

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
    var addressing by remember { mutableStateOf(false) }
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
                "Zones",
                style = MaterialTheme.typography.titleLarge,
                modifier = Modifier.padding(horizontal = 24.dp),
            )
            Text(
                "Each zone plays a queue of its own on the output it was added for",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(horizontal = 24.dp, vertical = 4.dp),
            )
            Column(Modifier.selectableGroup()) {
                zones.forEach { zone ->
                    ZoneRow(
                        zone = zone,
                        output = ZonePlan.heardOn(zone.rendererName, outputNamed(outputs, zone.device)),
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
                "Add a zone",
                style = MaterialTheme.typography.titleSmall,
                modifier = Modifier.padding(horizontal = 24.dp, vertical = 8.dp),
            )
            withSystemDefault(outputs).forEach { output ->
                val title = outputTitle(output)
                ListItem(
                    onClick = { features.zones.add(title, output) },
                    colors = ListItemDefaults.colors(containerColor = Color.Transparent),
                    leadingContent = { Icon(Icons.Rounded.Add, contentDescription = null) },
                ) {
                    Text("Add a zone on $title")
                }
            }
            Spacer(Modifier.height(8.dp))
            Text(
                "Network renderers",
                style = MaterialTheme.typography.titleSmall,
                modifier = Modifier.padding(horizontal = 24.dp, vertical = 8.dp),
            )
            ListItem(
                onClick = { features.renderers.look(!looking) },
                colors = ListItemDefaults.colors(containerColor = Color.Transparent),
                supportingContent = {
                    Text(
                        if (looking) "${renderers.size} found on this network"
                        else "Streamers and amplifiers on this network, as outputs for a zone"
                    )
                },
                trailingContent = { Switch(checked = looking, onCheckedChange = null) },
            ) {
                Text("Find renderers")
            }
            ListItem(
                onClick = { addressing = true },
                colors = ListItemDefaults.colors(containerColor = Color.Transparent),
                leadingContent = { Icon(Icons.Rounded.Add, contentDescription = null) },
                supportingContent = {
                    Text(
                        if (addresses.isEmpty()) "For a network that hides its renderers from a search"
                        else "${addresses.size} added"
                    )
                },
            ) {
                Text("Add a renderer by address")
            }
        }
    }
    if (addressing) {
        RendererAddressDialog(
            addresses = addresses,
            add = { features.renderers.add(it) },
            forget = { features.renderers.forget(it) },
            dismiss = { addressing = false },
        )
    }
    choosing?.let { zone ->
        RendererDialog(
            zone = zone,
            renderers = renderers,
            reaching = looking || addresses.isNotEmpty(),
            choose = { features.zones.playOn(zone.id, it.node) },
            dismiss = { choosing = null },
        )
    }
    renaming?.let { zone ->
        RenameZoneDialog(zone, save = { features.zones.rename(zone.id, it) }, dismiss = { renaming = null })
    }
    removing?.let { zone ->
        ConfirmDialog(
            title = "Remove ${zone.name}?",
            text = "Its queue goes with it and it stops playing",
            confirm = "Remove",
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
        supportingContent = { Text(ZonePlan.line(output, zone.transport, zone.playing)) },
        trailingContent = {
            Box {
                IconButton(onClick = { menu = true }) {
                    Icon(Icons.Rounded.MoreVert, contentDescription = "${zone.name} options")
                }
                DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                    if (mayHandOver) {
                        DropdownMenuItem(
                            text = { Text("Hand the song over here") },
                            onClick = {
                                menu = false
                                onHandOver()
                            },
                        )
                    }
                    DropdownMenuItem(
                        text = { Text("Play on a renderer") },
                        onClick = {
                            menu = false
                            onChooseRenderer()
                        },
                    )
                    if (zone.renderer != null) {
                        DropdownMenuItem(
                            text = { Text("Play on this phone") },
                            onClick = {
                                menu = false
                                onPlayOnPhone()
                            },
                        )
                    }
                    DropdownMenuItem(
                        text = { Text("Rename") },
                        onClick = {
                            menu = false
                            onRename()
                        },
                    )
                    if (ZonePlan.mayRemove(zone.id)) {
                        DropdownMenuItem(
                            text = { Text("Remove") },
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
        title = { Text("Rename zone") },
        text = {
            OutlinedTextField(
                value = name,
                onValueChange = { name = it.take(ZONE_NAME_AT_MOST) },
                label = { Text("Zone name") },
                singleLine = true,
                keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done),
                keyboardActions = KeyboardActions(onDone = { saved() }),
                modifier = Modifier.focusRequester(focus),
            )
        },
        confirmButton = { TextButton(onClick = saved, enabled = name.isNotBlank()) { Text("Save") } },
        dismissButton = { TextButton(onClick = dismiss) { Text("Cancel") } },
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
        title = { Text("Play ${zone.name} on") },
        text = {
            when {
                !reaching && renderers.isEmpty() ->
                    Text("Turn on Find renderers, or add one by its address, to reach renderers")
                renderers.isEmpty() -> Text("No renderer has answered yet")
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
                                supportingContent = { Text(renderer.host) },
                            ) {
                                Text(renderer.name)
                            }
                        }
                    }
            }
        },
        confirmButton = { TextButton(onClick = dismiss) { Text("Close") } },
    )
}

@Composable
private fun RendererAddressDialog(
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
        title = { Text("Add a renderer by address") },
        text = {
            Column(Modifier.verticalScroll(rememberScrollState())) {
                OutlinedTextField(
                    value = typed,
                    onValueChange = { typed = it.take(RENDERER_ADDRESS_AT_MOST) },
                    label = { Text("Description address") },
                    placeholder = { Text("http://192.168.1.20:49152/description.xml") },
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
                            TextButton(onClick = { forget(address) }) { Text("Remove") }
                        },
                    ) {
                        Text(address)
                    }
                }
            }
        },
        confirmButton = { TextButton(onClick = added, enabled = typed.isNotBlank()) { Text("Add") } },
        dismissButton = { TextButton(onClick = dismiss) { Text("Close") } },
    )
}

private const val RENDERER_ADDRESS_AT_MOST = 512
private const val ZONE_NAME_AT_MOST = 64
private const val RENDERERS_READ_EVERY_MS = 2_000L
