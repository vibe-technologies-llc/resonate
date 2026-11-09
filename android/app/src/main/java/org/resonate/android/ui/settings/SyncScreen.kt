package org.resonate.android.ui.settings

import android.os.SystemClock
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.selection.toggleable
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.QueueMusic
import androidx.compose.material.icons.rounded.Autorenew
import androidx.compose.material.icons.rounded.Close
import androidx.compose.material.icons.rounded.CloudOff
import androidx.compose.material.icons.rounded.CloudSync
import androidx.compose.material.icons.rounded.Computer
import androidx.compose.material.icons.rounded.ContentCopy
import androidx.compose.material.icons.rounded.CreateNewFolder
import androidx.compose.material.icons.rounded.SignalCellularAlt
import androidx.compose.material.icons.rounded.DeleteOutline
import androidx.compose.material.icons.rounded.Devices
import androidx.compose.material.icons.rounded.Download
import androidx.compose.material.icons.rounded.ErrorOutline
import androidx.compose.material.icons.rounded.Favorite
import androidx.compose.material.icons.rounded.Folder
import androidx.compose.material.icons.rounded.GroupAdd
import androidx.compose.material.icons.rounded.Key
import androidx.compose.material.icons.rounded.LibraryMusic
import androidx.compose.material.icons.rounded.PhoneAndroid
import androidx.compose.material.icons.rounded.PhoneIphone
import androidx.compose.material.icons.rounded.PlayCircle
import androidx.compose.material.icons.rounded.Sync
import androidx.compose.material.icons.rounded.Visibility
import androidx.compose.material.icons.rounded.VisibilityOff
import androidx.compose.material.icons.rounded.Wifi
import androidx.compose.material3.Badge
import androidx.compose.material3.Button
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearWavyProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.getValue
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext
import org.resonate.android.MusicDocuments
import org.resonate.android.MusicFolder
import org.resonate.android.R
import org.resonate.android.nativecore.DevicePlatform
import org.resonate.android.nativecore.RemoteControl
import org.resonate.android.nativecore.SyncConfig
import org.resonate.android.nativecore.syncControlOffered
import org.resonate.android.nativecore.SyncStanding
import org.resonate.android.nativecore.SyncStatus
import org.resonate.android.nativecore.SyncedDevice
import org.resonate.android.ui.Resonate
import org.resonate.android.ui.SYNC_ASKED_SHOWS_FOR_MS
import org.resonate.android.ui.SyncActivity
import org.resonate.android.ui.components.Page
import org.resonate.android.ui.components.SectionHeader
import org.resonate.android.ui.components.Sizes
import org.resonate.android.ui.navigation.LocalNavigator
import org.resonate.android.ui.navigation.Screen
import org.resonate.android.ui.resonateViewModel
import org.resonate.android.ui.syncActivity
import org.resonate.android.words

class SyncViewModel(private val resonate: Resonate) : ViewModel() {
    private val features = resonate.features

    val config = features.syncConfig
    val status = features.syncStatus
    val folder = features.syncFolder
    val folders = features.folders
    val notice = features.syncNotice
    val online = features.online
    val askedAt = resonate.syncAskedAt
    val inBackground = features.syncsInBackground

    fun syncInBackground(enabled: Boolean) = features.setSyncInBackground(enabled)

    fun refreshFolders() = features.refreshFolders()

    fun change(edit: (SyncConfig) -> SyncConfig) {
        config.value?.let { features.saveSync(edit(it)) }
    }

    val nameForOthers = features.together.nameForOthers

    fun saveNames(device: String, server: String, forOthers: String) {
        change { it.copy(device = device.trim(), server = server.trim()) }
        if (forOthers.trim() != nameForOthers.value) features.together.nameForOthers(forOthers)
    }

    fun join(code: String) = features.joinSync(code)

    fun newGroup() = features.newSyncGroup()

    fun choose(folder: MusicFolder) = features.chooseSyncFolder(folder)

    fun syncNow() = resonate.forceSync(viewModelScope)

    val drivenBy = resonate

    fun press(device: String, zone: UInt, control: RemoteControl) =
        resonate.attempt(viewModelScope) { native ->
            val answer = withContext(Dispatchers.IO) { native.syncControl(device, zone, control) }
            DrivenWords.refusal(resonate.library.words, answer)?.let { resonate.library.notice(it) }
        }

    fun forget(device: SyncedDevice) =
        resonate.attempt(viewModelScope) { native ->
            withContext(Dispatchers.IO) { native.syncForgetDevice(device.id) }
        }
}

private fun iconFor(device: SyncedDevice): ImageVector =
    when (device.platform) {
        DevicePlatform.ANDROID -> Icons.Rounded.PhoneAndroid
        DevicePlatform.IOS -> Icons.Rounded.PhoneIphone
        DevicePlatform.LINUX, DevicePlatform.WINDOWS -> Icons.Rounded.Computer
        DevicePlatform.OTHER -> Icons.Rounded.Devices
    }

private fun standingIcon(status: SyncStatus?, config: SyncConfig, online: Boolean): ImageVector =
    when {
        !config.enabled -> Icons.Rounded.CloudOff
        !online && !config.nearbyOnly -> Icons.Rounded.CloudOff
        status?.standing == SyncStanding.REFUSED || status?.standing == SyncStanding.UNREACHABLE ->
            Icons.Rounded.ErrorOutline
        config.nearbyOnly -> Icons.Rounded.Wifi
        else -> Icons.Rounded.CloudSync
    }

private fun destinationOf(folders: List<MusicFolder>, chosen: String?): MusicFolder? {
    val writable = folders.filter { it.readable && it.writable }
    return writable.find { it.uri.toString() == chosen }
        ?: writable.firstOrNull { it.uri != MusicDocuments.downloads }
}

private enum class Asking {
    JOIN,
    NEW_GROUP,
}

@Composable
fun SyncScreen() {
    val model = resonateViewModel { SyncViewModel(it) }
    val navigator = LocalNavigator.current
    val stored by model.config.collectAsStateWithLifecycle()
    val status by model.status.collectAsStateWithLifecycle()
    val chosen by model.folder.collectAsStateWithLifecycle()
    val folders by model.folders.collectAsStateWithLifecycle()
    val notice by model.notice.collectAsStateWithLifecycle()
    val online by model.online.collectAsStateWithLifecycle()
    val askedAt by model.askedAt.collectAsStateWithLifecycle()
    val inBackground by model.inBackground.collectAsStateWithLifecycle()
    val keptNameForOthers by model.nameForOthers.collectAsStateWithLifecycle()
    val offered = remember { syncControlOffered() }
    var showCode by remember { mutableStateOf(false) }
    var asking by rememberSaveable { mutableStateOf<Asking?>(null) }
    var forgetting by rememberSaveable { mutableStateOf<String?>(null) }
    var driving by rememberSaveable { mutableStateOf<String?>(null) }
    var joinCode by rememberSaveable { mutableStateOf("") }
    val copy = rememberCopy()
    val words = words()
    val pairingCode = stringResource(R.string.sync_pairing_code)
    LaunchedEffect(Unit) { model.refreshFolders() }
    Page(stringResource(R.string.sync_title)) { padding ->
        val config = stored
        if (config == null) {
            Loading(padding)
            return@Page
        }
        var device by rememberSaveable(config.device) { mutableStateOf(config.device) }
        var server by rememberSaveable(config.server) { mutableStateOf(config.server) }
        var forOthers by rememberSaveable(keptNameForOthers) { mutableStateOf(keptNameForOthers) }
        LaunchedEffect(config.key) { showCode = false }
        val code = status?.code?.ifEmpty { null } ?: config.key
        val devices = orderedDevices(status?.devices.orEmpty())
        val transfers = status?.transfers.orEmpty()
        val destination = destinationOf(folders, chosen)
        val writable = folders.filter { it.readable && it.writable }
        SettingsList(padding, Modifier.testTag("sync-list")) {
            item(key = "main") {
                MainSwitch(
                    title = stringResource(R.string.sync_use),
                    checked = config.enabled,
                    onCheckedChange = { value -> model.change { it.copy(enabled = value) } },
                )
            }
            item(key = "status") {
                StatusRow(config, status, online, askedAt, onSyncNow = model::syncNow) {
                    model.change { it.copy(nearbyOnly = true) }
                }
            }
            notice?.let { said ->
                item(key = "notice") {
                    NoticeCard(
                        Icons.Rounded.ErrorOutline,
                        said,
                        error = true,
                        action = stringResource(R.string.sync_notice_folders),
                        onAction = { navigator.open(Screen.Folders) },
                    )
                }
            }
            if (transfers.isNotEmpty()) {
                item(key = "transfers-header") {
                    SectionHeader(
                        transfersHeader(words, status?.downloading ?: 0u, status?.waiting ?: 0u)
                    )
                }
                transfers.forEachIndexed { index, transfer ->
                    item(key = "transfer-$index") {
                        val progress = transferProgress(transfer)
                        SettingRow(
                            title = transfer.title.ifBlank { stringResource(R.string.sync_a_song) },
                            icon = Icons.Rounded.Download,
                            supporting = transferLine(words, transfer),
                        )
                        Box(Modifier.fillMaxWidth().padding(start = 56.dp, end = Sizes.gutter, bottom = 8.dp)) {
                            if (progress != null)
                                LinearWavyProgressIndicator(
                                    progress = { progress },
                                    modifier = Modifier.fillMaxWidth(),
                                )
                            else LinearWavyProgressIndicator(Modifier.fillMaxWidth())
                        }
                    }
                }
            }
            item(key = "devices-header") {
                SectionHeader(stringResource(R.string.sync_devices_header))
            }
            if (devices.isEmpty()) {
                item(key = "no-devices") {
                    Note(
                        stringResource(
                            if (config.enabled) R.string.sync_no_devices
                            else R.string.sync_turn_on_to_see
                        )
                    )
                }
            }
            devices.forEach { found ->
                item(key = "device-${found.id}") {
                    DeviceRow(
                        found,
                        driven = driving == found.id,
                        onDrive = { driving = if (driving == found.id) null else found.id },
                    ) {
                        forgetting = found.id
                    }
                }
                if (driving == found.id && mayDrive(found)) {
                    item(key = "driven-${found.id}") {
                        DrivenDevice(model.drivenBy, found.id) { zone, control ->
                            model.press(found.id, zone, control)
                        }
                    }
                }
            }
            item(key = "pairing-header") {
                SectionHeader(stringResource(R.string.sync_pairing_header))
            }
            item(key = "pairing") {
                SettingRow(
                    title = pairingCode,
                    icon = Icons.Rounded.Key,
                    supporting = stringResource(R.string.sync_pairing_code_detail),
                )
                if (showCode && code.isNotEmpty()) {
                    CodeText(codeLines(code), Modifier.testTag("pairing-code"))
                }
                ButtonRow {
                    TextButton(
                        onClick = { showCode = !showCode },
                        enabled = code.isNotEmpty(),
                        modifier = Modifier.testTag("show-pairing-code"),
                    ) {
                        Icon(
                            if (showCode) Icons.Rounded.VisibilityOff else Icons.Rounded.Visibility,
                            contentDescription = null,
                        )
                        Spacer(Modifier.width(8.dp))
                        Text(
                            stringResource(
                                if (showCode) R.string.sync_hide_code else R.string.sync_show_code
                            )
                        )
                    }
                    OutlinedButton(
                        onClick = { copy(pairingCode, code) },
                        enabled = code.isNotEmpty(),
                    ) {
                        Icon(Icons.Rounded.ContentCopy, contentDescription = null)
                        Spacer(Modifier.width(8.dp))
                        Text(stringResource(R.string.sync_copy))
                    }
                }
            }
            item(key = "join") {
                Field(
                    value = joinCode,
                    onValueChange = { joinCode = it },
                    label = stringResource(R.string.sync_join_code),
                    secret = true,
                    modifier = Modifier.testTag("join-code"),
                )
                ButtonRow {
                    TextButton(onClick = { asking = Asking.NEW_GROUP }) {
                        Text(stringResource(R.string.sync_new_group))
                    }
                    FilledTonalButton(
                        onClick = { asking = Asking.JOIN },
                        enabled = joinCode.isNotBlank() && joinCode.trim() != code,
                        modifier = Modifier.testTag("join"),
                    ) {
                        Text(stringResource(R.string.sync_join))
                    }
                }
            }
            item(key = "follow-header") {
                SectionHeader(stringResource(R.string.sync_follow_header))
            }
            item(key = "favourites") {
                SwitchRow(
                    title = stringResource(R.string.sync_favourites),
                    icon = Icons.Rounded.Favorite,
                    supporting = stringResource(R.string.sync_favourites_detail),
                    checked = config.favourites,
                    onCheckedChange = { value -> model.change { it.copy(favourites = value) } },
                )
            }
            item(key = "playlists") {
                SwitchRow(
                    title = stringResource(R.string.sync_playlists),
                    icon = Icons.AutoMirrored.Rounded.QueueMusic,
                    supporting = stringResource(R.string.sync_playlists_detail),
                    checked = config.playlists,
                    onCheckedChange = { value -> model.change { it.copy(playlists = value) } },
                )
            }
            item(key = "everything") {
                SwitchRow(
                    title = stringResource(R.string.sync_everything),
                    icon = Icons.Rounded.LibraryMusic,
                    supporting = stringResource(R.string.sync_everything_detail),
                    checked = config.everything,
                    onCheckedChange = { value -> model.change { it.copy(everything = value) } },
                )
            }
            item(key = "metered") {
                SwitchRow(
                    title = stringResource(R.string.sync_metered),
                    icon = Icons.Rounded.SignalCellularAlt,
                    supporting = stringResource(R.string.sync_metered_detail),
                    checked = config.meteredAllowed,
                    onCheckedChange = { value -> model.change { it.copy(meteredAllowed = value) } },
                )
            }
            item(key = "background") {
                SwitchRow(
                    title = stringResource(R.string.sync_background),
                    icon = Icons.Rounded.Autorenew,
                    supporting = stringResource(R.string.sync_background_says),
                    checked = inBackground,
                    onCheckedChange = model::syncInBackground,
                )
            }
            item(key = "destination-header") {
                SectionHeader(stringResource(R.string.sync_destination_header))
            }
            if (writable.isEmpty()) {
                item(key = "no-destination") {
                    SettingRow(
                        title = stringResource(R.string.sync_no_destination),
                        icon = Icons.Rounded.CreateNewFolder,
                        supporting = stringResource(R.string.sync_no_destination_detail),
                        onClick = { navigator.open(Screen.Folders) },
                    )
                }
            } else {
                item(key = "destinations") {
                    if (destination == null) {
                        Note(stringResource(R.string.sync_choose_destination))
                    }
                    val chosenForYou = stringResource(R.string.sync_chosen_for_you)
                    Column(Modifier.selectableGroup()) {
                        writable.forEach { target ->
                            RadioRow(
                                title = target.name,
                                icon =
                                    if (target.uri == MusicDocuments.downloads) Icons.Rounded.Download
                                    else Icons.Rounded.Folder,
                                supporting =
                                    if (chosen == null && target == destination) chosenForYou
                                    else null,
                                selected = target == destination,
                                onClick = { model.choose(target) },
                            )
                        }
                    }
                    Footnote(stringResource(R.string.sync_destination_note))
                }
            }
            item(key = "connection-header") {
                SectionHeader(stringResource(R.string.sync_connection_header))
            }
            item(key = "nearby") {
                SwitchRow(
                    title = stringResource(R.string.sync_nearby),
                    icon = Icons.Rounded.Wifi,
                    supporting = stringResource(R.string.sync_nearby_detail),
                    checked = config.nearbyOnly,
                    onCheckedChange = { value -> model.change { it.copy(nearbyOnly = value) } },
                )
            }
            item(key = "names") {
                Field(
                    value = device,
                    onValueChange = { device = it },
                    label = stringResource(R.string.sync_device_name),
                    supporting = stringResource(R.string.sync_device_name_detail),
                    modifier = Modifier.testTag("device-name"),
                )
                Field(
                    value = forOthers,
                    onValueChange = { forOthers = it },
                    label = stringResource(R.string.sync_name_for_others),
                    supporting = stringResource(R.string.sync_name_for_others_detail),
                    modifier = Modifier.testTag("name-for-others"),
                )
                if (!config.nearbyOnly) {
                    Field(
                        value = server,
                        onValueChange = { server = it },
                        label = stringResource(R.string.sync_server),
                        supporting = stringResource(R.string.sync_server_detail),
                        keyboardType = androidx.compose.ui.text.input.KeyboardType.Uri,
                        modifier = Modifier.testTag("sync-server"),
                    )
                }
                val unsaved =
                    namesUnsaved(config, device, server) || forOthers.trim() != keptNameForOthers
                ButtonRow {
                    Button(onClick = { model.saveNames(device, server, forOthers) }, enabled = unsaved) {
                        Text(
                            stringResource(if (unsaved) R.string.sync_save else R.string.sync_saved)
                        )
                    }
                }
            }
            item(key = "control-header") { SectionHeader(offered.title) }
            item(key = "control") {
                Note(offered.asked)
                Column(Modifier.selectableGroup()) {
                    offered.choices.forEach { choice ->
                        RadioRow(
                            title = choice.label,
                            supporting = choice.meaning,
                            selected = config.control == choice.control,
                            onClick = { model.change { it.copy(control = choice.control) } },
                            modifier = Modifier.testTag("sync-control-${choice.label}"),
                        )
                    }
                }
                Footnote(offered.told)
            }
            item(key = "keystore") {
                Footnote(stringResource(R.string.sync_keystore))
            }
        }
    }
    when (asking) {
        Asking.JOIN ->
            ConfirmDialog(
                title = stringResource(R.string.sync_join_title),
                text = stringResource(R.string.sync_join_text),
                confirm = stringResource(R.string.sync_join),
                icon = Icons.Rounded.GroupAdd,
                onConfirm = {
                    model.join(joinCode)
                    joinCode = ""
                },
                onDismiss = { asking = null },
            )
        Asking.NEW_GROUP ->
            ConfirmDialog(
                title = stringResource(R.string.sync_new_group_title),
                text = stringResource(R.string.sync_new_group_text),
                confirm = stringResource(R.string.sync_new_group_confirm),
                icon = Icons.Rounded.GroupAdd,
                onConfirm = model::newGroup,
                onDismiss = { asking = null },
            )
        null -> Unit
    }
    val leaving = status?.devices?.firstOrNull { it.id == forgetting }
    if (leaving != null) {
        ConfirmDialog(
            title = stringResource(R.string.sync_forget_title, leaving.name),
            text = stringResource(R.string.sync_forget_text),
            confirm = stringResource(R.string.sync_forget),
            icon = Icons.Rounded.DeleteOutline,
            destructive = true,
            onConfirm = { model.forget(leaving) },
            onDismiss = { forgetting = null },
        )
    }
}

@Composable
private fun MainSwitch(title: String, checked: Boolean, onCheckedChange: (Boolean) -> Unit) {
    val scheme = MaterialTheme.colorScheme
    Surface(
        color = if (checked) scheme.primaryContainer else scheme.surfaceContainerHighest,
        contentColor = if (checked) scheme.onPrimaryContainer else scheme.onSurface,
        shape = MaterialTheme.shapes.extraLarge,
        modifier =
            Modifier.fillMaxWidth()
                .padding(horizontal = Sizes.gutter, vertical = 8.dp)
                .testTag("sync-enabled"),
    ) {
        Row(
            Modifier.toggleable(value = checked, role = Role.Switch, onValueChange = onCheckedChange)
                .padding(horizontal = 24.dp, vertical = 20.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(title, style = MaterialTheme.typography.titleLarge, modifier = Modifier.weight(1f))
            Switch(checked = checked, onCheckedChange = null)
        }
    }
}

@Composable
private fun StatusRow(
    config: SyncConfig,
    status: SyncStatus?,
    online: Boolean,
    askedAt: Long?,
    onSyncNow: () -> Unit,
    onNearbyOnly: () -> Unit,
) {
    val now by
        produceState(SystemClock.elapsedRealtime(), askedAt) {
            value = SystemClock.elapsedRealtime()
            val asked = askedAt ?: return@produceState
            val left = asked + SYNC_ASKED_SHOWS_FOR_MS - value
            if (left > 0) {
                delay(left)
                value = SystemClock.elapsedRealtime()
            }
        }
    val activity = syncActivity(status, askedAt, now)
    val words = words()
    val waitingForOnline = config.enabled && !online && !config.nearbyOnly
    SettingRow(
        title = syncHeadline(words, status, config, online),
        supporting = syncStanding(words, status, config, online),
        icon = if (activity == null) standingIcon(status, config, online) else null,
        leadingContent = if (activity != null) ({ SmallLoading() }) else null,
        modifier = Modifier.semantics { liveRegion = LiveRegionMode.Polite }.testTag("sync-status"),
    )
    if (config.enabled) {
        ButtonRow(Modifier.padding(start = 40.dp), start = true) {
            if (waitingForOnline) {
                FilledTonalButton(onClick = onNearbyOnly) {
                    Icon(Icons.Rounded.Wifi, contentDescription = null, modifier = Modifier.size(18.dp))
                    Spacer(Modifier.width(8.dp))
                    Text(stringResource(R.string.sync_nearby_only_action))
                }
            } else {
                FilledTonalButton(onClick = onSyncNow, modifier = Modifier.testTag("sync-now")) {
                    Icon(Icons.Rounded.Sync, contentDescription = null, modifier = Modifier.size(18.dp))
                    Spacer(Modifier.width(8.dp))
                    Text(stringResource(R.string.sync_now))
                }
            }
        }
    }
    if (activity is SyncActivity.Fetching) {
        Box(Modifier.fillMaxWidth().padding(start = 56.dp, end = Sizes.gutter, bottom = 8.dp)) {
            val fraction = activity.fraction
            if (fraction != null)
                LinearWavyProgressIndicator(progress = { fraction }, modifier = Modifier.fillMaxWidth())
            else LinearWavyProgressIndicator(Modifier.fillMaxWidth())
        }
    }
}

@Composable
private fun DeviceRow(
    device: SyncedDevice,
    driven: Boolean,
    onDrive: () -> Unit,
    onForget: () -> Unit,
) {
    val scheme = MaterialTheme.colorScheme
    val lit = device.thisDevice || device.online || device.nearby
    val words = words()
    SettingRow(
        title = device.name.ifBlank { stringResource(R.string.sync_unnamed_device) },
        supporting = deviceState(words, device),
        leadingContent = {
            Box {
                Icon(
                    iconFor(device),
                    contentDescription = null,
                    tint = if (lit) scheme.primary else scheme.onSurfaceVariant,
                )
                if (!device.thisDevice && lit) {
                    Badge(
                        containerColor = scheme.primary,
                        modifier = Modifier.align(Alignment.BottomEnd).size(8.dp),
                    )
                }
            }
        },
        trailing =
            if (mayDrive(device)) {
                {
                    IconButton(onClick = onDrive) {
                        Icon(
                            if (driven) Icons.Rounded.Close else Icons.Rounded.PlayCircle,
                            contentDescription =
                                stringResource(
                                    if (driven) R.string.sync_stop_driving else R.string.sync_drive,
                                    device.name,
                                ),
                        )
                    }
                }
            } else if (mayForget(device)) {
                {
                    IconButton(onClick = onForget) {
                        Icon(
                            Icons.Rounded.DeleteOutline,
                            contentDescription =
                                stringResource(R.string.sync_forget_device, device.name),
                        )
                    }
                }
            } else null,
        modifier = Modifier.testTag("device"),
    )
}

private fun mayDrive(device: SyncedDevice): Boolean = !device.thisDevice && (device.online || device.nearby)
