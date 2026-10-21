package org.resonate.android.ui.settings

import org.resonate.android.ui.components.LocalSheets
import androidx.compose.material.icons.rounded.Hearing
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.AutoAwesome
import androidx.compose.material.icons.rounded.BarChart
import androidx.compose.material.icons.rounded.BookmarkBorder
import androidx.compose.material.icons.rounded.CheckCircle
import androidx.compose.material.icons.rounded.ContentCopy
import androidx.compose.material.icons.rounded.CloudOff
import androidx.compose.material.icons.rounded.ErrorOutline
import androidx.compose.material.icons.rounded.FolderOpen
import androidx.compose.material.icons.rounded.Fingerprint
import androidx.compose.material.icons.rounded.ImageSearch
import androidx.compose.material.icons.rounded.History
import androidx.compose.material.icons.rounded.Info
import androidx.compose.material.icons.rounded.Refresh
import androidx.compose.material.icons.rounded.Sync
import androidx.compose.material.icons.rounded.Wifi
import androidx.compose.material3.Button
import androidx.compose.material3.LinearWavyProgressIndicator
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.getValue
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.resonate.android.nativecore.EnrichmentStatus
import org.resonate.android.nativecore.EnrichmentStop
import org.resonate.android.ui.Resonate
import org.resonate.android.ui.components.Page
import org.resonate.android.ui.components.SectionHeader
import org.resonate.android.ui.components.Sizes
import org.resonate.android.ui.counted
import org.resonate.android.ui.navigation.LocalNavigator
import org.resonate.android.ui.navigation.Screen
import org.resonate.android.ui.resonateViewModel

class LibraryToolsViewModel(resonate: Resonate) : ViewModel() {
    private val features = resonate.features
    private val enrichment = features.enrichment
    private val library = resonate.library

    val online = features.online
    val automatic = enrichment.automatic
    val unmeteredOnly = enrichment.unmeteredOnly
    val status = enrichment.status
    val coversAskedAgain = enrichment.coversAskedAgain
    val recognitionKey = features.recognitionKey
    val auddToken = features.auddToken
    val rememberSearches = features.rememberSearches
    val scanning = library.scanning
    val songs = library.allTracks

    fun read() = enrichment.read()

    fun setOnline(enabled: Boolean) = features.setOnline(enabled)

    fun setAutomatic(enabled: Boolean) {
        enrichment.setAutomatic(enabled)
    }

    fun setUnmeteredOnly(enabled: Boolean) {
        enrichment.setUnmeteredOnly(enabled)
    }

    fun refreshAll() {
        enrichment.refresh()
    }

    fun stop() {
        enrichment.stop()
    }

    fun retryCovers() {
        enrichment.retryMissingCovers()
    }

    fun rescan() = library.rescan()

    fun saveRecognitionKey(value: String) = features.saveRecognitionKey(value.trim())

    fun saveAuddToken(value: String) = features.saveAuddToken(value)

    fun setRememberSearches(enabled: Boolean) = features.setRememberSearches(enabled)
}

private fun stoppedIcon(status: EnrichmentStatus?): ImageVector =
    when (status?.stopped) {
        EnrichmentStop.FINISHED -> Icons.Rounded.CheckCircle
        EnrichmentStop.UNREACHABLE,
        EnrichmentStop.REFUSED,
        EnrichmentStop.FAILED -> Icons.Rounded.ErrorOutline
        EnrichmentStop.OFFLINE -> Icons.Rounded.CloudOff
        else -> Icons.Rounded.Info
    }

@Composable
fun LibraryToolsScreen() {
    val model = resonateViewModel { LibraryToolsViewModel(it) }
    val navigator = LocalNavigator.current
    val online by model.online.collectAsStateWithLifecycle()
    val automatic by model.automatic.collectAsStateWithLifecycle()
    val unmeteredOnly by model.unmeteredOnly.collectAsStateWithLifecycle()
    val status by model.status.collectAsStateWithLifecycle()
    val covers by model.coversAskedAgain.collectAsStateWithLifecycle()
    val storedKey by model.recognitionKey.collectAsStateWithLifecycle()
    val auddToken by model.auddToken.collectAsStateWithLifecycle()
    val rememberSearches by model.rememberSearches.collectAsStateWithLifecycle()
    val scanning by model.scanning.collectAsStateWithLifecycle()
    val songs by model.songs.collectAsStateWithLifecycle()
    var confirmRefresh by rememberSaveable { mutableStateOf(false) }
    LaunchedEffect(Unit) { model.read() }
    val running = status?.running == true
    Page("Library tools") { padding ->
        SettingsList(padding, Modifier.testTag("tools-list")) {
            item(key = "artwork") { SectionHeader("Artwork and details") }
            if (!online) {
                item(key = "offline") {
                    NoticeCard(
                        Icons.Rounded.CloudOff,
                        "Online features are off, so nothing is looked up.",
                        action = "Turn on",
                        onAction = { model.setOnline(true) },
                    )
                }
            }
            item(key = "automatic") {
                SwitchRow(
                    title = "Fetch automatically",
                    icon = Icons.Rounded.AutoAwesome,
                    supporting = "Covers, artist portraits, lyrics and MusicBrainz details",
                    checked = automatic,
                    onCheckedChange = model::setAutomatic,
                    enabled = online,
                    modifier = Modifier.testTag("enrich-automatically"),
                )
            }
            item(key = "wifi") {
                SwitchRow(
                    title = "Only on Wi-Fi",
                    icon = Icons.Rounded.Wifi,
                    supporting = "Wait for a network that is not metered",
                    checked = unmeteredOnly,
                    onCheckedChange = model::setUnmeteredOnly,
                    enabled = online && automatic,
                )
            }
            item(key = "status") {
                EnrichmentState(status, running, onStop = model::stop)
            }
            item(key = "refresh") {
                SettingRow(
                    title = "Refresh artwork and details",
                    icon = Icons.Rounded.Refresh,
                    supporting = "Look everything up again for ${counted(songs.size, "song")}",
                    enabled = online && songs.isNotEmpty(),
                    onClick = { confirmRefresh = true },
                    modifier = Modifier.testTag("refresh-enrichment"),
                )
            }
            item(key = "covers") {
                SettingRow(
                    title = "Look again for missing covers",
                    icon = Icons.Rounded.ImageSearch,
                    supporting = coversLine(covers),
                    enabled = online,
                    onClick = model::retryCovers,
                )
            }
            item(key = "library") { SectionHeader("Your library") }
            item(key = "rescan") {
                SettingRow(
                    title = "Rescan folders",
                    icon = Icons.Rounded.Sync,
                    supporting =
                        if (scanning) "Reading your music folders…"
                        else "Find new, changed and removed songs",
                    enabled = !scanning,
                    onClick = model::rescan,
                    trailing = if (scanning) ({ SmallLoading() }) else null,
                )
            }
            item(key = "folders") {
                SettingRow(
                    title = "Music folders",
                    icon = Icons.Rounded.FolderOpen,
                    supporting = "Add, remove or check folder access",
                    onClick = { navigator.open(Screen.Folders) },
                )
            }
            item(key = "statistics") {
                SettingRow(
                    title = "Listening statistics",
                    icon = Icons.Rounded.BarChart,
                    supporting = "Your most played, and what your library holds",
                    onClick = { navigator.open(Screen.Statistics) },
                )
            }
            item(key = "suggestions") {
                SettingRow(
                    title = "Made for you",
                    icon = Icons.Rounded.AutoAwesome,
                    supporting = "Mixes from your music and listening",
                    onClick = { navigator.open(Screen.Suggestions) },
                )
            }
            item(key = "identical") {
                SettingRow(
                    title = "Identical songs",
                    icon = Icons.Rounded.ContentCopy,
                    supporting = "Find songs stored twice and keep one file",
                    onClick = { navigator.open(Screen.IdenticalSongs) },
                    modifier = Modifier.testTag("identical-songs"),
                )
            }
            item(key = "listen") {
                SettingRow(
                    title = "Listen",
                    icon = Icons.Rounded.Hearing,
                    supporting = "Name the song playing around you",
                    onClick = LocalSheets.current::listen,
                    modifier = Modifier.testTag("listen-tool"),
                )
            }
            item(key = "wanted") {
                SettingRow(
                    title = "Saved for later",
                    icon = Icons.Rounded.BookmarkBorder,
                    supporting = "Recordings you want to find",
                    onClick = { navigator.open(Screen.Wanted) },
                )
            }
            item(key = "recognition") { SectionHeader("Recognition") }
            item(key = "acoustid") { RecognitionKey(storedKey, model::saveRecognitionKey) }
            item(key = "audd") { AuddToken(auddToken, model::saveAuddToken) }
            item(key = "search") { SectionHeader("Search") }
            item(key = "remember-searches") {
                SwitchRow(
                    title = "Remember recent searches",
                    checked = rememberSearches,
                    onCheckedChange = model::setRememberSearches,
                    icon = Icons.Rounded.History,
                    supporting = "Turning this off also clears the searches kept so far",
                    modifier = Modifier.testTag("remember-searches"),
                )
            }
        }
    }
    if (confirmRefresh) {
        ConfirmDialog(
            title = "Refresh artwork and details?",
            text =
                "Covers, artist portraits, details and lyrics will be looked up again for ${counted(songs.size, "song")}. This runs in the background and can take a while.",
            confirm = "Refresh",
            icon = Icons.Rounded.Refresh,
            onConfirm = model::refreshAll,
            onDismiss = { confirmRefresh = false },
        )
    }
}

@Composable
private fun EnrichmentState(status: EnrichmentStatus?, running: Boolean, onStop: () -> Unit) {
    val progress = enrichmentProgress(status)
    SettingRow(
        title = enrichmentLine(status),
        supporting = enrichmentFound(status),
        icon = if (running) null else stoppedIcon(status),
        trailing =
            if (running) {
                { OutlinedButton(onClick = onStop) { Text("Stop") } }
            } else null,
        leadingContent = if (running) ({ SmallLoading() }) else null,
        modifier = Modifier.testTag("enrichment-status"),
    )
    if (running) {
        Box(
            Modifier.fillMaxWidth().padding(start = 56.dp, end = Sizes.gutter, bottom = 12.dp),
            contentAlignment = Alignment.CenterStart,
        ) {
            if (progress != null) {
                LinearWavyProgressIndicator(
                    progress = { progress },
                    modifier =
                        Modifier.fillMaxWidth().semantics {
                            contentDescription = "Artwork lookup progress"
                        },
                )
            } else {
                LinearWavyProgressIndicator(Modifier.fillMaxWidth())
            }
        }
    }
}

@Composable
private fun RecognitionKey(stored: String, onSave: (String) -> Unit) {
    var draft by remember(stored) { mutableStateOf(stored) }
    SettingRow(
        title = "AcoustID key",
        icon = Icons.Rounded.Fingerprint,
        supporting =
            "With a key, Recognise in a song's analysis sends its AcoustID fingerprint. Without one, it sends a Shazam signature.",
    )
    Field(
        value = draft,
        onValueChange = { draft = it },
        label = "AcoustID application key",
        supporting = "Optional · kept encrypted on this device",
        secret = true,
        modifier = Modifier.testTag("acoustid-key"),
    )
    ButtonRow {
        if (stored.isNotEmpty()) {
            OutlinedButton(
                onClick = {
                    draft = ""
                    onSave("")
                }
            ) {
                Text("Forget key")
            }
        }
        Button(onClick = { onSave(draft) }, enabled = draft.trim() != stored) { Text("Save key") }
    }
}

@Composable
private fun AuddToken(stored: String, onSave: (String) -> Unit) {
    var draft by remember(stored) { mutableStateOf(stored) }
    SettingRow(
        title = "AudD token",
        icon = Icons.Rounded.Hearing,
        supporting = "Listen asks AudD when Shazam does not know a song. Without a token only Shazam is asked.",
    )
    Field(
        value = draft,
        onValueChange = { draft = it },
        label = "AudD API token",
        supporting = "Optional · kept encrypted on this device",
        secret = true,
        modifier = Modifier.testTag("audd-token"),
    )
    ButtonRow {
        if (stored.isNotEmpty()) {
            OutlinedButton(
                onClick = {
                    draft = ""
                    onSave("")
                }
            ) {
                Text("Forget token")
            }
        }
        Button(onClick = { onSave(draft) }, enabled = draft.trim() != stored) { Text("Save token") }
    }
}
