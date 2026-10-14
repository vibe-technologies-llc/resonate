package org.resonate.android

import android.content.Intent
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowRight
import androidx.compose.material.icons.automirrored.filled.ManageSearch
import androidx.compose.material.icons.automirrored.filled.PlaylistPlay
import androidx.compose.material.icons.filled.AutoAwesome
import androidx.compose.material.icons.filled.Bedtime
import androidx.compose.material.icons.filled.Clear
import androidx.compose.material.icons.filled.CloudQueue
import androidx.compose.material.icons.filled.Devices
import androidx.compose.material.icons.filled.Download
import androidx.compose.material.icons.filled.Equalizer
import androidx.compose.material.icons.filled.FolderOpen
import androidx.compose.material.icons.filled.GraphicEq
import androidx.compose.material.icons.filled.Headphones
import androidx.compose.material.icons.filled.Insights
import androidx.compose.material.icons.filled.Lyrics
import androidx.compose.material.icons.filled.NotificationsNone
import androidx.compose.material.icons.filled.Search
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.filled.Speaker
import androidx.compose.material.icons.filled.Sync
import androidx.compose.material.icons.filled.Tune
import androidx.compose.material.icons.filled.Waves
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.core.app.NotificationManagerCompat
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import org.resonate.android.nativecore.GainMode
import org.resonate.android.nativecore.MusicProvider
import org.resonate.android.nativecore.Snapshot

private data class SettingsEntry(
    val title: String,
    val description: String,
    val icon: ImageVector,
    val keywords: String = "",
    val enabled: Boolean = true,
    val action: () -> Unit,
)

@Composable
fun SettingsScreen(
    library: LibraryRepository,
    snapshot: Snapshot?,
    navigate: (String) -> Unit,
    openSleep: () -> Unit,
    allowNotifications: () -> Unit,
) {
    val preferences by library.preferences.collectAsState()
    val folders by library.features.folders.collectAsState()
    val scanning by library.scanning.collectAsState()
    val provider by library.features.providerAccount.collectAsState()
    val accounts by library.features.scrobbleAccount.collectAsState()
    val syncConfig by library.features.syncConfig.collectAsState()
    val syncStatus by library.features.syncStatus.collectAsState()
    val onlineEnabled by library.features.online.collectAsState()
    var query by rememberSaveable { mutableStateOf("") }
    val context = LocalContext.current
    val focus = LocalFocusManager.current
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    var notificationsAllowed by remember {
        mutableStateOf(NotificationManagerCompat.from(context).areNotificationsEnabled())
    }
    DisposableEffect(lifecycle) {
        val observer = LifecycleEventObserver { _, event ->
            if (event == Lifecycle.Event.ON_RESUME)
                notificationsAllowed =
                    NotificationManagerCompat.from(context).areNotificationsEnabled()
        }
        lifecycle.addObserver(observer)
        onDispose { lifecycle.removeObserver(observer) }
    }
    LaunchedEffect(Unit) { library.features.refreshFolders() }
    val sound =
        listOf(
            SettingsEntry(
                "Equaliser",
                if (preferences?.equaliserEnabled == true)
                    preferences?.profileName?.ifBlank { "Custom ten-band curve" }.orEmpty()
                else "Ten-band curve and preamp",
                Icons.Default.Equalizer,
                "sound gains bass treble profile",
            ) {
                navigate("Sound")
            },
            SettingsEntry(
                "Sound processing",
                when (preferences?.replayGain) {
                    GainMode.TRACK -> "Track ReplayGain"
                    GainMode.ALBUM -> "Album ReplayGain"
                    else -> "ReplayGain, resampling and clip prevention"
                },
                Icons.Default.GraphicEq,
                "loudness true peak filter phase quality restoration DSP",
            ) {
                navigate("Processing")
            },
            SettingsEntry(
                "Audio output",
                "Choose a device and adjust buffering",
                Icons.Default.Speaker,
                "route bluetooth USB DAC buffer DSD",
            ) {
                navigate("Output")
            },
            SettingsEntry(
                "Headphone correction",
                "Find and apply an AutoEq profile",
                Icons.Default.Headphones,
                "equaliser correction",
            ) {
                navigate("AutoEq")
            },
            SettingsEntry(
                "Sleep timer",
                sleepDescription(snapshot),
                Icons.Default.Bedtime,
                "pause minutes end song queue",
            ) {
                openSleep()
            },
        )
    val sources =
        listOf(
            SettingsEntry(
                "Music folders",
                if (folders.isEmpty()) "Add and manage your music folders"
                else
                    "${folders.size} ${if (folders.size == 1) "folder" else "folders"} · ${folders.count { it.readable }} available",
                Icons.Default.FolderOpen,
                "library sources storage add remove rescan",
            ) {
                navigate("Folders")
            },
            SettingsEntry(
                "Rescan library",
                if (scanning) "Reading your music…"
                else if (folders.none { it.readable }) "Add a music folder first"
                else "Find new and changed music in your folders",
                Icons.Default.Sync,
                "refresh scan files",
                !scanning && folders.any { it.readable },
            ) {
                library.rescan()
            },
            SettingsEntry(
                "Notifications",
                if (notificationsAllowed) "Manage playback and download notifications"
                else "Allow playback and download notifications",
                Icons.Default.NotificationsNone,
                "media controls alerts",
            ) {
                if (notificationsAllowed)
                    context.startActivity(
                        Intent(android.provider.Settings.ACTION_APP_NOTIFICATION_SETTINGS)
                            .putExtra(
                                android.provider.Settings.EXTRA_APP_PACKAGE,
                                context.packageName,
                            )
                    )
                else allowNotifications()
            },
        )
    val connected =
        listOf(
            SettingsEntry(
                "Download providers",
                when (provider?.provider) {
                    MusicProvider.SUBSONIC -> "Subsonic"
                    MusicProvider.TIDAL -> "TIDAL"
                    MusicProvider.HIFI_API -> "hifi-api"
                    else -> "Monochrome"
                },
                Icons.Default.Download,
                "account server TIDAL Subsonic Monochrome hifi-api",
            ) {
                navigate("Download providers")
            },
            SettingsEntry(
                "Scrobbling",
                listOfNotNull(
                        "ListenBrainz".takeIf { accounts?.listenbrainzToken?.isNotBlank() == true },
                        "Last.fm".takeIf { accounts?.lastfmSession?.isNotBlank() == true },
                    )
                    .joinToString(" · ")
                    .ifBlank { "Connect ListenBrainz or Last.fm" },
                Icons.Default.Insights,
                "accounts listening history favourites",
            ) {
                navigate("Scrobbling")
            },
            SettingsEntry(
                "Device sync",
                syncStandingText(syncStatus, syncConfig, onlineEnabled),
                Icons.Default.Devices,
                "pairing code phone computer playlists favourites listening history handoff",
            ) {
                navigate("Device sync")
            },
        )
    val terms = query.trim().split(Regex("\\s+")).filter { it.isNotEmpty() }
    fun matches(value: String) = terms.all { value.contains(it, ignoreCase = true) }
    val sections =
        listOf(
                "Sound & playback" to sound,
                "Music library" to sources,
                "Connected services" to connected,
            )
            .map { (title, entries) ->
                title to
                    entries.filter {
                        matches("$title ${it.title} ${it.description} ${it.keywords}")
                    }
            }
            .filter { it.second.isNotEmpty() }
    val showOnline =
        matches("Online features internet network artwork covers lyrics enrichment music search")
    LazyColumn(
        Modifier.fillMaxSize(),
        contentPadding = PaddingValues(start = 16.dp, end = 16.dp, bottom = 24.dp),
        verticalArrangement = Arrangement.spacedBy(20.dp),
    ) {
        item(key = "settings-search") {
            OutlinedTextField(
                query,
                { query = it },
                placeholder = { Text("Search settings") },
                leadingIcon = { Icon(Icons.Default.Search, null) },
                trailingIcon = {
                    if (query.isNotEmpty())
                        IconButton(onClick = { query = "" }) {
                            Icon(Icons.Default.Clear, "Clear settings search")
                        }
                },
                modifier =
                    Modifier.fillMaxWidth().semantics { contentDescription = "Search settings" },
                shape = RoundedCornerShape(20.dp),
                singleLine = true,
                keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done),
                keyboardActions = KeyboardActions(onDone = { focus.clearFocus() }),
            )
        }
        if (showOnline) item(key = "online") { OnlineSettings(library.features) }
        sections.forEach { (title, entries) ->
            item(key = title) { SettingsGroup(title, entries) }
        }
        if (sections.isEmpty() && !showOnline)
            item(key = "no-results") {
                Text(
                    "No settings match “$query”",
                    Modifier.padding(16.dp),
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
    }
}

@Composable
private fun OnlineSettings(features: LibraryFeatures) {
    val online by features.online.collectAsState()
    Surface(
        shape = RoundedCornerShape(24.dp),
        color = MaterialTheme.colorScheme.secondaryContainer,
    ) {
        Row(
            Modifier.fillMaxWidth()
                .toggleable(online, role = Role.Switch, onValueChange = features::setOnline)
                .padding(20.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            Icon(
                Icons.Default.CloudQueue,
                null,
                tint = MaterialTheme.colorScheme.onSecondaryContainer,
            )
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Text("Online features", style = MaterialTheme.typography.titleMedium)
                Text(
                    if (online) "Artwork, lyrics and music search are enabled"
                    else "Artwork, lyrics and music search are offline",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSecondaryContainer,
                )
            }
            Switch(checked = online, onCheckedChange = null)
        }
    }
}

@Composable
fun LibraryToolsScreen(
    library: LibraryRepository,
    hasTrack: Boolean,
    navigate: (String) -> Unit,
    openFilters: () -> Unit,
) {
    val tracks by library.tracks.collectAsState()
    val online by library.features.online.collectAsState()
    val enriching by library.features.enriching.collectAsState()
    val groups =
        listOf(
            "Your library" to
                listOf(
                    SettingsEntry(
                        "Playlists",
                        "Listen to your collections",
                        Icons.AutoMirrored.Filled.PlaylistPlay,
                    ) {
                        navigate("Playlists")
                    },
                    SettingsEntry(
                        "Suggestions",
                        "Rediscover music from your library",
                        Icons.Default.AutoAwesome,
                    ) {
                        navigate("Suggestions")
                    },
                    SettingsEntry(
                        "Statistics",
                        "Your listening history and most played music",
                        Icons.Default.Insights,
                    ) {
                        navigate("Statistics")
                    },
                    SettingsEntry(
                        "Filter and sort",
                        "Genre, artist, audio quality and more",
                        Icons.Default.Tune,
                    ) {
                        openFilters()
                    },
                ),
            "Discover" to
                listOf(
                    SettingsEntry(
                        "Online search",
                        "Find songs, artists, albums and shared music",
                        Icons.Default.Search,
                    ) {
                        navigate("Online search")
                    },
                    SettingsEntry(
                        "Saved for later",
                        "Recordings you want to add to your library",
                        Icons.Default.Download,
                    ) {
                        navigate("Saved for later")
                    },
                    SettingsEntry(
                        "Enrich visible music",
                        when {
                            enriching -> "Updating artwork and metadata…"
                            !online -> "Enable online features in Settings"
                            else -> "Look up covers, metadata and lyrics for ${tracks.size} songs"
                        },
                        Icons.Default.AutoAwesome,
                        enabled = online && tracks.isNotEmpty() && !enriching,
                    ) {
                        library.features.enrich(tracks)
                    },
                ),
            "Current song" to
                listOf(
                    SettingsEntry(
                        "Lyrics",
                        "Follow the words as they play",
                        Icons.Default.Lyrics,
                        enabled = hasTrack,
                    ) {
                        navigate("Lyrics")
                    },
                    SettingsEntry(
                        "Visualizer",
                        "Watch the live waveform",
                        Icons.Default.Waves,
                        enabled = hasTrack,
                    ) {
                        navigate("Visualizer")
                    },
                    SettingsEntry(
                        "Analysis",
                        "Loudness, spectrum and audio quality",
                        Icons.Default.GraphicEq,
                        enabled = hasTrack,
                    ) {
                        navigate("Analysis")
                    },
                    SettingsEntry(
                        "Inspector",
                        "Artwork, tags and source details",
                        Icons.AutoMirrored.Filled.ManageSearch,
                        enabled = hasTrack,
                    ) {
                        navigate("Inspector")
                    },
                ),
            "Preferences" to
                listOf(
                    SettingsEntry(
                        "Settings",
                        "Sound, music folders and connected services",
                        Icons.Default.Settings,
                    ) {
                        navigate("Settings")
                    }
                ),
        )
    LazyColumn(
        Modifier.fillMaxSize(),
        contentPadding = PaddingValues(16.dp),
        verticalArrangement = Arrangement.spacedBy(24.dp),
    ) {
        groups.forEach { (title, entries) -> item(key = title) { SettingsGroup(title, entries) } }
    }
}

@Composable
private fun SettingsGroup(title: String, entries: List<SettingsEntry>) {
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Text(
            title,
            Modifier.padding(start = 16.dp),
            style = MaterialTheme.typography.labelLarge,
            color = MaterialTheme.colorScheme.primary,
        )
        Surface(
            shape = RoundedCornerShape(24.dp),
            color = MaterialTheme.colorScheme.surfaceContainer,
        ) {
            Column {
                entries.forEachIndexed { index, entry ->
                    SettingsRow(entry)
                    if (index < entries.lastIndex) SettingsDivider()
                }
            }
        }
    }
}

@Composable
private fun SettingsRow(entry: SettingsEntry) {
    Row(
        Modifier.fillMaxWidth()
            .clickable(enabled = entry.enabled, role = Role.Button, onClick = entry.action)
            .padding(horizontal = 16.dp, vertical = 16.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        Surface(
            shape = RoundedCornerShape(14.dp),
            color =
                MaterialTheme.colorScheme.secondaryContainer.copy(
                    alpha = if (entry.enabled) 1f else 0.4f
                ),
        ) {
            Icon(
                entry.icon,
                null,
                Modifier.padding(10.dp).size(22.dp),
                tint =
                    if (entry.enabled) MaterialTheme.colorScheme.onSecondaryContainer
                    else MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Text(
                entry.title,
                style = MaterialTheme.typography.titleMedium,
                color =
                    if (entry.enabled) MaterialTheme.colorScheme.onSurface
                    else MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Text(
                entry.description,
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
        }
        Icon(
            Icons.AutoMirrored.Filled.KeyboardArrowRight,
            null,
            Modifier.size(20.dp),
            tint = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

@Composable
private fun SettingsDivider() {
    HorizontalDivider(
        Modifier.padding(start = 74.dp, end = 16.dp),
        color = MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.5f),
    )
}
