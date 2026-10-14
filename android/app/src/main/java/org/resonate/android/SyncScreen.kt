package org.resonate.android

import android.content.ClipData
import android.content.ClipboardManager
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ContentCopy
import androidx.compose.material.icons.filled.Devices
import androidx.compose.material3.AssistChip
import androidx.compose.material3.Button
import androidx.compose.material3.FilterChip
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import org.resonate.android.nativecore.DevicePlatform
import org.resonate.android.nativecore.SyncConfig
import org.resonate.android.nativecore.SyncStanding
import org.resonate.android.nativecore.SyncStatus
import org.resonate.android.nativecore.syncReadsAsACode

private const val HOSTED_SYNC_SERVER = "resonate.tecnio.dev"

fun syncStandingText(status: SyncStatus?, config: SyncConfig?, online: Boolean): String =
    when {
        config?.enabled != true -> "Off"
        !online -> "Waiting for online features"
        else ->
            when (status?.standing) {
                SyncStanding.CONNECTED -> {
                    val others = status?.devices.orEmpty().count { !it.thisDevice && it.online }
                    if (others == 0) "Connected · no other device online"
                    else "Connected · $others ${if (others == 1) "device" else "devices"} online"
                }
                SyncStanding.UNREACHABLE -> "The sync server could not be reached"
                SyncStanding.REFUSED -> "The sync server refused this device"
                SyncStanding.CONNECTING -> "Connecting…"
                else -> "Starting…"
            }
    }

@Composable
fun SyncScreen(library: LibraryRepository) {
    val features = library.features
    val saved by features.syncConfig.collectAsState()
    val status by features.syncStatus.collectAsState()
    val online by features.online.collectAsState()
    val folders by features.folders.collectAsState()
    val chosenFolder by features.syncFolder.collectAsState()
    val notice by features.syncNotice.collectAsState()
    val config = saved ?: return
    var joining by rememberSaveable { mutableStateOf("") }
    var device by remember(config.device) { mutableStateOf(config.device) }
    var server by remember(config.server) { mutableStateOf(config.server) }
    val context = LocalContext.current
    LaunchedEffect(Unit) { features.refreshFolders() }
    Column(
        Modifier.verticalScroll(rememberScrollState()).padding(20.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        Text("Device sync", style = MaterialTheme.typography.headlineSmall)
        Text(
            "Playlists, favourites, listening history and what is playing follow you between your devices. Records are sealed with your pairing code before they leave this phone.",
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        SyncSwitch(
            "Sync with your other devices",
            syncStandingText(status, config, online),
            config.enabled,
        ) {
            features.saveSync(config.copy(enabled = it))
        }
        if (!online)
            Text(
                "Turn on online features in Settings to sync.",
                color = MaterialTheme.colorScheme.error,
            )
        notice?.let { Text(it, color = MaterialTheme.colorScheme.error) }
        SyncSection("Pairing code") {
            val code = status?.code?.ifBlank { null } ?: config.key.ifBlank { null }
            Row(verticalAlignment = Alignment.CenterVertically) {
                SelectionContainer(Modifier.weight(1f)) {
                    Text(
                        code ?: "Made when sync first starts",
                        style = MaterialTheme.typography.titleLarge,
                        fontFamily = FontFamily.Monospace,
                    )
                }
                if (code != null)
                    IconButton(
                        onClick = {
                            context
                                .getSystemService(ClipboardManager::class.java)
                                .setPrimaryClip(ClipData.newPlainText("Resonate pairing code", code))
                        }
                    ) {
                        Icon(Icons.Default.ContentCopy, "Copy pairing code")
                    }
            }
            Text(
                "Enter this code on another device to join it to this one.",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            val reads = joining.isNotBlank() && syncReadsAsACode(joining.trim())
            OutlinedTextField(
                joining,
                { joining = it },
                label = { Text("Join with a code from another device") },
                isError = joining.isNotBlank() && !reads,
                supportingText = {
                    if (joining.isNotBlank() && !reads) Text("This is not a pairing code")
                },
                modifier = Modifier.fillMaxWidth(),
                singleLine = true,
            )
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Button(
                    onClick = {
                        features.joinSync(joining)
                        joining = ""
                    },
                    enabled = reads,
                ) {
                    Text("Join")
                }
                TextButton(onClick = { features.newSyncGroup() }) {
                    Text("Leave and make a new code")
                }
            }
        }
        SyncSection("This device") {
            OutlinedTextField(
                device,
                { device = it },
                label = { Text("Device name") },
                placeholder = { Text("Android") },
                modifier = Modifier.fillMaxWidth(),
                singleLine = true,
            )
            OutlinedTextField(
                server,
                { server = it },
                label = { Text("Sync server") },
                placeholder = { Text(HOSTED_SYNC_SERVER) },
                supportingText = { Text("Leave blank to use $HOSTED_SYNC_SERVER") },
                modifier = Modifier.fillMaxWidth(),
                singleLine = true,
            )
            Button(
                onClick = {
                    features.saveSync(config.copy(device = device.trim(), server = server.trim()))
                },
                enabled = device.trim() != config.device || server.trim() != config.server,
            ) {
                Text("Save")
            }
        }
        SyncSection("Songs fetched from your devices") {
            SyncSwitch(
                "Favourites",
                "Fetch every song favoured on another device",
                config.favourites,
            ) {
                features.saveSync(config.copy(favourites = it))
            }
            SyncSwitch(
                "Kept playlists",
                "Fetch the songs of playlists kept on this device",
                config.playlists,
            ) {
                features.saveSync(config.copy(playlists = it))
            }
            SyncSwitch(
                "Everything",
                "Fetch every song your other devices hold",
                config.everything,
            ) {
                features.saveSync(config.copy(everything = it))
            }
            SyncSwitch(
                "Allow mobile data for songs",
                "Without it, songs move only on unmetered networks",
                config.meteredAllowed,
            ) {
                features.saveSync(config.copy(meteredAllowed = it))
            }
            val destinations = folders.filter { it.readable && it.writable }
            Text(
                "Songs from your devices are kept in",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            if (destinations.isEmpty()) Text("Add a music folder with write access first.")
            val kept =
                destinations.find { it.uri.toString() == chosenFolder } ?: destinations.firstOrNull()
            FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                destinations.forEach { folder ->
                    FilterChip(
                        kept == folder,
                        { features.chooseSyncFolder(folder) },
                        label = { Text(folder.name) },
                    )
                }
            }
        }
        val devices = status?.devices.orEmpty()
        if (devices.isNotEmpty())
            SyncSection("Devices") {
                devices.forEachIndexed { index, synced ->
                    Row(
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(16.dp),
                    ) {
                        Icon(
                            Icons.Default.Devices,
                            null,
                            tint =
                                if (synced.online) MaterialTheme.colorScheme.primary
                                else MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                        Column(Modifier.weight(1f)) {
                            Text(synced.name, style = MaterialTheme.typography.titleMedium)
                            Text(
                                listOfNotNull(
                                        when (synced.platform) {
                                            DevicePlatform.LINUX -> "Linux"
                                            DevicePlatform.ANDROID -> "Android"
                                            DevicePlatform.IOS -> "iOS"
                                            DevicePlatform.OTHER -> null
                                        },
                                        if (synced.online) "online" else "away",
                                        "this device".takeIf { synced.thisDevice },
                                    )
                                    .joinToString(" · "),
                                style = MaterialTheme.typography.bodyMedium,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                    }
                    if (index < devices.lastIndex)
                        HorizontalDivider(
                            color = MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.5f)
                        )
                }
            }
        val moving = status?.moving ?: 0u
        if (moving > 0u)
            Text(
                "Moving $moving ${if (moving == 1u) "song" else "songs"} between your devices",
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
    }
}

@Composable
private fun SyncSection(title: String, content: @Composable () -> Unit) {
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Text(
            title,
            Modifier.padding(start = 4.dp),
            style = MaterialTheme.typography.labelLarge,
            color = MaterialTheme.colorScheme.primary,
        )
        Surface(
            shape = RoundedCornerShape(24.dp),
            color = MaterialTheme.colorScheme.surfaceContainer,
        ) {
            Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                content()
            }
        }
    }
}

@Composable
private fun SyncSwitch(
    title: String,
    description: String,
    checked: Boolean,
    change: (Boolean) -> Unit,
) {
    Row(
        Modifier.fillMaxWidth().toggleable(checked, role = Role.Switch, onValueChange = change),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Text(title, style = MaterialTheme.typography.titleMedium)
            Text(
                description,
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        Switch(checked = checked, onCheckedChange = null)
    }
}

@Composable
fun HandedOffRow(library: LibraryRepository, play: () -> Unit) {
    val handoffs by library.features.syncHandoffs.collectAsState()
    if (handoffs.isEmpty()) return
    Row(
        Modifier.fillMaxWidth()
            .horizontalScroll(rememberScrollState())
            .padding(horizontal = 16.dp, vertical = 8.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        handoffs.forEach { handoff ->
            AssistChip(
                onClick = { library.features.continueFrom(handoff, play) },
                label = {
                    Text(
                        "Continue from ${handoff.device} · ${handoff.songs} ${if (handoff.songs == 1u) "song" else "songs"}"
                    )
                },
                leadingIcon = { Icon(Icons.Default.Devices, null) },
            )
        }
    }
}
