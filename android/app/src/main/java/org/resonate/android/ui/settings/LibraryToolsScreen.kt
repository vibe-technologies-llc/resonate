package org.resonate.android.ui.settings

import org.resonate.android.ui.components.LocalSheets
import androidx.compose.material.icons.rounded.Hearing
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.Undo
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
import androidx.compose.material.icons.rounded.Person
import androidx.compose.material.icons.rounded.Refresh
import androidx.compose.material.icons.rounded.SignalCellularAlt
import androidx.compose.material.icons.rounded.Sync
import androidx.compose.material.icons.rounded.Wifi
import androidx.compose.material3.Button
import androidx.compose.material3.LinearWavyProgressIndicator
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
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
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.compose.foundation.lazy.items
import org.resonate.android.FollowedProfileWords
import org.resonate.android.R
import org.resonate.android.nativecore.EnrichmentStatus
import org.resonate.android.nativecore.EnrichmentStop
import org.resonate.android.ui.Resonate
import org.resonate.android.ui.components.Page
import org.resonate.android.ui.components.SectionHeader
import org.resonate.android.ui.components.Sizes
import org.resonate.android.ui.navigation.LocalNavigator
import org.resonate.android.ui.navigation.Screen
import org.resonate.android.ui.resonateViewModel
import org.resonate.android.words

class LibraryToolsViewModel(private val resonate: Resonate) : ViewModel() {
    private val features = resonate.features
    private val enrichment = features.enrichment
    private val library = resonate.library

    val online = features.online
    val automatic = enrichment.automatic
    val unmeteredOnly = enrichment.unmeteredOnly
    val enrichOnMetered = features.enrichOnMetered
    val status = enrichment.status
    val coversAskedAgain = enrichment.coversAskedAgain
    val recognitionKey = features.recognitionKey
    val auddToken = features.auddToken
    val rememberSearches = features.rememberSearches
    val tellsFindings = features.tellsFindings
    val tagsUndoable = features.tags.undoable
    val scanning = library.scanning
    val songs = library.allTracks
    val organiseUndo = features.organising.undo
    val organising = features.organising.busy
    val profilesFollowed = features.profilesFollowed

    fun read() {
        enrichment.read()
        features.loadProfilesFollowed()
    }

    fun unfollow(link: String) = features.unfollowProfile(link)

    fun setOnline(enabled: Boolean) = features.setOnline(enabled)

    fun setAutomatic(enabled: Boolean) {
        enrichment.setAutomatic(enabled)
    }

    fun setUnmeteredOnly(enabled: Boolean) {
        enrichment.setUnmeteredOnly(enabled)
    }

    fun setEnrichOnMetered(enabled: Boolean) = features.setEnrichOnMetered(enabled)

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

    fun putFilesBack() = features.organising.putBack()

    fun undoTags() = resonate.undoTags()

    fun saveRecognitionKey(value: String) = features.saveRecognitionKey(value.trim())

    fun saveAuddToken(value: String) = features.saveAuddToken(value)

    fun setRememberSearches(enabled: Boolean) = features.setRememberSearches(enabled)

    fun setTellsFindings(enabled: Boolean) = features.setTellsFindings(enabled)
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
    val enrichOnMetered by model.enrichOnMetered.collectAsStateWithLifecycle()
    val status by model.status.collectAsStateWithLifecycle()
    val covers by model.coversAskedAgain.collectAsStateWithLifecycle()
    val storedKey by model.recognitionKey.collectAsStateWithLifecycle()
    val auddToken by model.auddToken.collectAsStateWithLifecycle()
    val rememberSearches by model.rememberSearches.collectAsStateWithLifecycle()
    val tellsFindings by model.tellsFindings.collectAsStateWithLifecycle()
    val scanning by model.scanning.collectAsStateWithLifecycle()
    val songs by model.songs.collectAsStateWithLifecycle()
    val organiseUndo by model.organiseUndo.collectAsStateWithLifecycle()
    val organising by model.organising.collectAsStateWithLifecycle()
    val tagsUndoable by model.tagsUndoable.collectAsStateWithLifecycle()
    val profilesFollowed by model.profilesFollowed.collectAsStateWithLifecycle()
    var unfollowing by rememberSaveable { mutableStateOf<String?>(null) }
    var confirmRefresh by rememberSaveable { mutableStateOf(false) }
    LaunchedEffect(Unit) { model.read() }
    val running = status?.running == true
    Page(stringResource(R.string.tools_title)) { padding ->
        SettingsList(padding, Modifier.testTag("tools-list")) {
            item(key = "artwork") { SectionHeader(stringResource(R.string.tools_artwork_header)) }
            if (!online) {
                item(key = "offline") {
                    NoticeCard(
                        Icons.Rounded.CloudOff,
                        stringResource(R.string.tools_offline),
                        action = stringResource(R.string.tools_turn_on),
                        onAction = { model.setOnline(true) },
                    )
                }
            }
            item(key = "automatic") {
                SwitchRow(
                    title = stringResource(R.string.tools_automatic),
                    icon = Icons.Rounded.AutoAwesome,
                    supporting = stringResource(R.string.tools_automatic_detail),
                    checked = automatic,
                    onCheckedChange = model::setAutomatic,
                    enabled = online,
                    modifier = Modifier.testTag("enrich-automatically"),
                )
            }
            item(key = "wifi") {
                SwitchRow(
                    title = stringResource(R.string.tools_wifi),
                    icon = Icons.Rounded.Wifi,
                    supporting = stringResource(R.string.tools_wifi_detail),
                    checked = unmeteredOnly,
                    onCheckedChange = model::setUnmeteredOnly,
                    enabled = online && automatic,
                )
            }
            item(key = "metered") {
                SwitchRow(
                    title = stringResource(R.string.tools_metered),
                    icon = Icons.Rounded.SignalCellularAlt,
                    supporting = stringResource(R.string.tools_metered_detail),
                    checked = enrichOnMetered,
                    onCheckedChange = model::setEnrichOnMetered,
                    enabled = online,
                    modifier = Modifier.testTag("setting-enrich-on-metered"),
                )
            }
            item(key = "status") {
                EnrichmentState(status, running, onStop = model::stop)
            }
            item(key = "refresh") {
                SettingRow(
                    title = stringResource(R.string.tools_refresh),
                    icon = Icons.Rounded.Refresh,
                    supporting =
                        pluralStringResource(
                            R.plurals.tools_refresh_detail,
                            songs.size,
                            songs.size,
                        ),
                    enabled = online && songs.isNotEmpty(),
                    onClick = { confirmRefresh = true },
                    modifier = Modifier.testTag("refresh-enrichment"),
                )
            }
            item(key = "covers") {
                SettingRow(
                    title = stringResource(R.string.tools_covers),
                    icon = Icons.Rounded.ImageSearch,
                    supporting = coversLine(words(), covers),
                    enabled = online,
                    onClick = model::retryCovers,
                )
            }
            item(key = "library") { SectionHeader(stringResource(R.string.tools_library_header)) }
            item(key = "rescan") {
                SettingRow(
                    title = stringResource(R.string.tools_rescan),
                    icon = Icons.Rounded.Sync,
                    supporting =
                        stringResource(
                            if (scanning) R.string.tools_rescan_reading
                            else R.string.tools_rescan_detail
                        ),
                    enabled = !scanning,
                    onClick = model::rescan,
                    trailing = if (scanning) ({ SmallLoading() }) else null,
                )
            }
            item(key = "tell-findings") {
                SwitchRow(
                    title = stringResource(R.string.tools_tell_findings),
                    checked = tellsFindings,
                    onCheckedChange = model::setTellsFindings,
                    icon = Icons.Rounded.Info,
                    supporting = stringResource(R.string.tools_tell_findings_detail),
                    modifier = Modifier.testTag("tell-findings"),
                )
            }
            item(key = "folders") {
                SettingRow(
                    title = stringResource(R.string.tools_folders),
                    icon = Icons.Rounded.FolderOpen,
                    supporting = stringResource(R.string.tools_folders_detail),
                    onClick = { navigator.open(Screen.Folders) },
                )
            }
            if (organiseUndo != null) {
                item(key = "put-back") {
                    SettingRow(
                        title = stringResource(R.string.tools_put_back),
                        icon = Icons.AutoMirrored.Rounded.Undo,
                        supporting = stringResource(R.string.tools_put_back_detail),
                        enabled = !organising,
                        onClick = model::putFilesBack,
                        modifier = Modifier.testTag("put-files-back"),
                    )
                }
            }
            item(key = "statistics") {
                SettingRow(
                    title = stringResource(R.string.tools_statistics),
                    icon = Icons.Rounded.BarChart,
                    supporting = stringResource(R.string.tools_statistics_detail),
                    onClick = { navigator.open(Screen.Statistics) },
                )
            }
            item(key = "suggestions") {
                SettingRow(
                    title = stringResource(R.string.tools_suggestions),
                    icon = Icons.Rounded.AutoAwesome,
                    supporting = stringResource(R.string.tools_suggestions_detail),
                    onClick = { navigator.open(Screen.Suggestions) },
                )
            }
            item(key = "identical") {
                SettingRow(
                    title = stringResource(R.string.tools_identical),
                    icon = Icons.Rounded.ContentCopy,
                    supporting = stringResource(R.string.tools_identical_detail),
                    onClick = { navigator.open(Screen.IdenticalSongs) },
                    modifier = Modifier.testTag("identical-songs"),
                )
            }
            if (tagsUndoable) {
                item(key = "undo-tags") {
                    SettingRow(
                        title = stringResource(R.string.tools_undo_tags),
                        icon = Icons.AutoMirrored.Rounded.Undo,
                        supporting = stringResource(R.string.tools_undo_tags_detail),
                        onClick = model::undoTags,
                        modifier = Modifier.testTag("undo-tags"),
                    )
                }
            }
            item(key = "listen") {
                SettingRow(
                    title = stringResource(R.string.tools_listen),
                    icon = Icons.Rounded.Hearing,
                    supporting = stringResource(R.string.tools_listen_detail),
                    onClick = LocalSheets.current::listen,
                    modifier = Modifier.testTag("listen-tool"),
                )
            }
            item(key = "wanted") {
                SettingRow(
                    title = stringResource(R.string.tools_wanted),
                    icon = Icons.Rounded.BookmarkBorder,
                    supporting = stringResource(R.string.tools_wanted_detail),
                    onClick = { navigator.open(Screen.Wanted) },
                )
            }
            if (profilesFollowed.isNotEmpty()) {
                item(key = "profiles") {
                    SectionHeader(stringResource(R.string.tools_profiles_header))
                }
                items(profilesFollowed, key = { "profile-${it.link}" }) { profile ->
                    SettingRow(
                        title = profile.name,
                        icon = Icons.Rounded.Person,
                        supporting = FollowedProfileWords.line(words(), profile.service),
                        trailing = {
                            TextButton(onClick = { unfollowing = profile.link }) {
                                Text(stringResource(R.string.tools_stop_following))
                            }
                        },
                        modifier = Modifier.testTag("followed-profile"),
                    )
                }
            }
            item(key = "recognition") {
                SectionHeader(stringResource(R.string.tools_recognition_header))
            }
            item(key = "acoustid") { RecognitionKey(storedKey, model::saveRecognitionKey) }
            item(key = "audd") { AuddToken(auddToken, model::saveAuddToken) }
            item(key = "search") { SectionHeader(stringResource(R.string.tools_search_header)) }
            item(key = "remember-searches") {
                SwitchRow(
                    title = stringResource(R.string.tools_remember_searches),
                    checked = rememberSearches,
                    onCheckedChange = model::setRememberSearches,
                    icon = Icons.Rounded.History,
                    supporting = stringResource(R.string.tools_remember_searches_detail),
                    modifier = Modifier.testTag("remember-searches"),
                )
            }
        }
    }
    unfollowing?.let { link ->
        ConfirmDialog(
            title = stringResource(R.string.tools_unfollow_title),
            text =
                stringResource(
                    R.string.tools_unfollow_text,
                    profilesFollowed.firstOrNull { it.link == link }?.name ?: link,
                ),
            confirm = stringResource(R.string.tools_stop_following),
            icon = Icons.Rounded.Person,
            onConfirm = { model.unfollow(link) },
            onDismiss = { unfollowing = null },
        )
    }
    if (confirmRefresh) {
        ConfirmDialog(
            title = stringResource(R.string.tools_refresh_title),
            text = pluralStringResource(R.plurals.tools_refresh_text, songs.size, songs.size),
            confirm = stringResource(R.string.tools_refresh_confirm),
            icon = Icons.Rounded.Refresh,
            onConfirm = model::refreshAll,
            onDismiss = { confirmRefresh = false },
        )
    }
}

@Composable
private fun EnrichmentState(status: EnrichmentStatus?, running: Boolean, onStop: () -> Unit) {
    val progress = enrichmentProgress(status)
    val progressWords = stringResource(R.string.tools_artwork_progress)
    SettingRow(
        title = enrichmentLine(words(), status),
        supporting = enrichmentFound(words(), status),
        icon = if (running) null else stoppedIcon(status),
        trailing =
            if (running) {
                { OutlinedButton(onClick = onStop) { Text(stringResource(R.string.tools_stop)) } }
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
                            contentDescription = progressWords
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
    var draft by rememberSaveable(stored) { mutableStateOf(stored) }
    SettingRow(
        title = stringResource(R.string.tools_acoustid),
        icon = Icons.Rounded.Fingerprint,
        supporting = stringResource(R.string.tools_acoustid_detail),
    )
    Field(
        value = draft,
        onValueChange = { draft = it },
        label = stringResource(R.string.tools_acoustid_label),
        supporting = stringResource(R.string.tools_kept_encrypted),
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
                Text(stringResource(R.string.tools_forget_key))
            }
        }
        Button(onClick = { onSave(draft) }, enabled = draft.trim() != stored) {
            Text(stringResource(R.string.tools_save_key))
        }
    }
}

@Composable
private fun AuddToken(stored: String, onSave: (String) -> Unit) {
    var draft by rememberSaveable(stored) { mutableStateOf(stored) }
    SettingRow(
        title = stringResource(R.string.tools_audd),
        icon = Icons.Rounded.Hearing,
        supporting = stringResource(R.string.tools_audd_detail),
    )
    Field(
        value = draft,
        onValueChange = { draft = it },
        label = stringResource(R.string.tools_audd_label),
        supporting = stringResource(R.string.tools_kept_encrypted),
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
                Text(stringResource(R.string.tools_forget_token))
            }
        }
        Button(onClick = { onSave(draft) }, enabled = draft.trim() != stored) {
            Text(stringResource(R.string.tools_save_token))
        }
    }
}
