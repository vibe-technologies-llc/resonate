package org.resonate.android.ui.settings

import android.net.Uri
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.CheckCircle
import androidx.compose.material.icons.rounded.CloudDownload
import androidx.compose.material.icons.rounded.CloudOff
import androidx.compose.material.icons.rounded.FileDownload
import androidx.compose.material.icons.rounded.FileUpload
import androidx.compose.material.icons.rounded.Favorite
import androidx.compose.material.icons.rounded.History
import androidx.compose.material.icons.rounded.Info
import androidx.compose.material.icons.rounded.Lock
import androidx.compose.material3.Button
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.lifecycle.ViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.resonate.android.R
import org.resonate.android.nativecore.ScrobbleAccount
import org.resonate.android.nativecore.ScrobbleService
import org.resonate.android.ui.Resonate
import org.resonate.android.ui.components.Page
import org.resonate.android.ui.components.SectionHeader
import org.resonate.android.ui.resonateViewModel

data class ScrobbleDraft(val account: ScrobbleAccount, val includeHistory: Boolean = false)

fun ScrobbleAccount.trimmed(): ScrobbleAccount =
    copy(
        listenbrainzToken = listenbrainzToken.trim(),
        lastfmKey = lastfmKey.trim(),
        lastfmSecret = lastfmSecret.trim(),
    )

fun canSignInToLastFm(draft: ScrobbleDraft): Boolean =
    draft.account.lastfmKey.isNotBlank() && draft.account.lastfmSecret.isNotBlank()

const val HISTORY_FILE_NAME = "listens.jsonl"

class ScrobblingViewModel(private val resonate: Resonate) : ViewModel() {
    private val features = resonate.features

    val lastfmAddress = features.lastfmAddress
    val lastfmBusy = features.lastfmBusy
    val stored = features.scrobbleAccount
    val status = features.scrobbleStatus
    val online = features.online
    val historyBusy = features.historyBusy
    val historyStatus = features.historyStatus
    var draft by mutableStateOf<ScrobbleDraft?>(null)
        private set

    init {
        viewModelScope.launch {
            var previous: ScrobbleAccount? = null
            stored.collect { account ->
                if (account == null) return@collect
                val before = previous
                previous = account
                val current = draft
                draft =
                    when {
                        current == null -> ScrobbleDraft(account)
                        before?.lastfmSession != account.lastfmSession ->
                            current.copy(
                                account = current.account.copy(lastfmSession = account.lastfmSession)
                            )
                        else -> current
                    }
            }
        }
    }

    fun edit(change: (ScrobbleDraft) -> ScrobbleDraft) {
        draft = draft?.let(change)
    }

    fun save() {
        val current = draft ?: return
        features.saveScrobbling(current.account.trimmed(), current.includeHistory)
        edit { it.copy(includeHistory = false) }
    }

    fun validate() {
        draft?.let { features.validateScrobbling(it.account.trimmed()) }
    }

    fun signIn() {
        val current = draft ?: return
        features.signInLastFm(current.account.trimmed())
    }

    fun cancelSignIn() =
        resonate.attempt(viewModelScope) { native ->
            withContext(Dispatchers.IO) { native.cancelLastfmAuthorization() }
        }

    fun open(link: String) = resonate.host.open(Uri.parse(link))

    fun signOut() = features.signOutLastFm()

    fun goOnline() = features.setOnline(true)

    fun exportHistory(uri: Uri) = features.exportHistory(uri)

    fun importHistoryFile(uri: Uri) = features.importHistoryFile(uri)

    fun importHistory(service: ScrobbleService) = features.importHistory(service)
}

@Composable
fun ScrobblingScreen() {
    val model = resonateViewModel { ScrobblingViewModel(it) }
    val stored by model.stored.collectAsStateWithLifecycle()
    val draft = model.draft
    val status by model.status.collectAsStateWithLifecycle()
    val online by model.online.collectAsStateWithLifecycle()
    val lastfmAddress by model.lastfmAddress.collectAsStateWithLifecycle()
    val lastfmBusy by model.lastfmBusy.collectAsStateWithLifecycle()
    val historyBusy by model.historyBusy.collectAsStateWithLifecycle()
    val historyStatus by model.historyStatus.collectAsStateWithLifecycle()
    val exporting =
        rememberLauncherForActivityResult(
            ActivityResultContracts.CreateDocument("application/octet-stream")
        ) { uri ->
            uri?.let(model::exportHistory)
        }
    val importing =
        rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
            uri?.let(model::importHistoryFile)
        }
    Page(stringResource(R.string.scrobbling_title)) { padding ->
        val current = draft
        if (current == null) {
            Loading(padding)
            return@Page
        }
        val account = current.account
        val unsaved =
            stored?.let { it.trimmed() != account.trimmed() } ?: true || current.includeHistory
        SettingsList(padding, Modifier.testTag("scrobbling-list")) {
            item(key = "intro") {
                Note(stringResource(R.string.scrobbling_intro))
            }
            if (!online) {
                item(key = "offline") {
                    NoticeCard(
                        Icons.Rounded.CloudOff,
                        stringResource(R.string.scrobbling_offline),
                        action = stringResource(R.string.scrobbling_turn_on),
                        onAction = model::goOnline,
                    )
                }
            }
            item(key = "listenbrainz-header") { SectionHeader("ListenBrainz") }
            item(key = "listenbrainz") {
                Field(
                    value = account.listenbrainzToken,
                    onValueChange = { value ->
                        model.edit { it.copy(account = it.account.copy(listenbrainzToken = value)) }
                    },
                    label = stringResource(R.string.scrobbling_user_token),
                    supporting = stringResource(R.string.scrobbling_user_token_detail),
                    secret = true,
                    modifier = Modifier.testTag("listenbrainz-token"),
                )
            }
            item(key = "lastfm-header") { SectionHeader("Last.fm") }
            item(key = "lastfm-key") {
                Field(
                    value = account.lastfmKey,
                    onValueChange = { value ->
                        model.edit { it.copy(account = it.account.copy(lastfmKey = value)) }
                    },
                    label = stringResource(R.string.scrobbling_api_key),
                    secret = true,
                )
            }
            item(key = "lastfm-secret") {
                Field(
                    value = account.lastfmSecret,
                    onValueChange = { value ->
                        model.edit { it.copy(account = it.account.copy(lastfmSecret = value)) }
                    },
                    label = stringResource(R.string.scrobbling_shared_secret),
                    supporting = stringResource(R.string.scrobbling_shared_secret_detail),
                    secret = true,
                )
            }
            if (account.lastfmSession.isNotEmpty()) {
                item(key = "lastfm-session") {
                    SettingRow(
                        title = stringResource(R.string.scrobbling_lastfm_signed_in),
                        icon = Icons.Rounded.CheckCircle,
                        supporting = stringResource(R.string.scrobbling_lastfm_signed_in_detail),
                        trailing = {
                            TextButton(onClick = model::signOut) {
                                Text(stringResource(R.string.scrobbling_sign_out))
                            }
                        },
                    )
                }
            } else {
                item(key = "lastfm-signed-out") {
                    SettingRow(
                        title = stringResource(R.string.scrobbling_lastfm_sign_in_title),
                        icon = Icons.Rounded.Lock,
                        supporting =
                            stringResource(
                                if (lastfmBusy) R.string.scrobbling_lastfm_waiting_detail
                                else R.string.scrobbling_lastfm_sign_in_detail
                            ),
                        modifier = Modifier.semantics { liveRegion = LiveRegionMode.Polite },
                    )
                }
                item(key = "lastfm-sign-in") {
                    ButtonRow {
                        val waiting = lastfmAddress
                        if (lastfmBusy) {
                            TextButton(
                                onClick = { model.cancelSignIn() },
                                modifier = Modifier.testTag("lastfm-cancel"),
                            ) {
                                Text(stringResource(R.string.scrobbling_lastfm_cancel))
                            }
                            if (waiting != null) {
                                OutlinedButton(onClick = { model.open(waiting) }) {
                                    Text(stringResource(R.string.scrobbling_lastfm_open_again))
                                }
                            }
                        } else {
                            FilledTonalButton(
                                onClick = model::signIn,
                                enabled = online && canSignInToLastFm(current),
                                modifier = Modifier.testTag("lastfm-sign-in"),
                            ) {
                                Text(stringResource(R.string.scrobbling_sign_in))
                            }
                        }
                    }
                }
            }
            item(key = "options-header") {
                SectionHeader(stringResource(R.string.scrobbling_when_saving))
            }
            item(key = "history") {
                SwitchRow(
                    title = stringResource(R.string.scrobbling_history),
                    icon = Icons.Rounded.History,
                    supporting = stringResource(R.string.scrobbling_history_detail),
                    checked = current.includeHistory,
                    onCheckedChange = { value -> model.edit { it.copy(includeHistory = value) } },
                )
            }
            item(key = "actions") {
                ButtonRow {
                    OutlinedButton(onClick = model::validate, enabled = online) {
                        Text(stringResource(R.string.scrobbling_check))
                    }
                    Button(
                        onClick = model::save,
                        enabled = unsaved,
                        modifier = Modifier.testTag("save-scrobbling"),
                    ) {
                        Text(
                            stringResource(
                                if (unsaved) R.string.scrobbling_save else R.string.scrobbling_saved
                            )
                        )
                    }
                }
            }
            status?.let { said ->
                item(key = "status") {
                    SettingRow(
                        title = said,
                        icon = Icons.Rounded.Info,
                        modifier = Modifier.semantics { liveRegion = LiveRegionMode.Polite },
                    )
                }
            }
            item(key = "loves") {
                SettingRow(
                    title = stringResource(R.string.scrobbling_loves),
                    icon = Icons.Rounded.Favorite,
                    supporting = stringResource(R.string.scrobbling_loves_detail),
                )
            }
            item(key = "history-header") {
                SectionHeader(stringResource(R.string.scrobbling_listening_history))
            }
            item(key = "history-export") {
                SettingRow(
                    title = stringResource(R.string.scrobbling_history_export),
                    icon = Icons.Rounded.FileUpload,
                    supporting = stringResource(R.string.scrobbling_history_export_detail),
                    enabled = !historyBusy,
                    onClick = { exporting.launch(HISTORY_FILE_NAME) },
                    modifier = Modifier.testTag("history-export"),
                )
            }
            item(key = "history-import-file") {
                SettingRow(
                    title = stringResource(R.string.scrobbling_history_import_file),
                    icon = Icons.Rounded.FileDownload,
                    supporting = stringResource(R.string.scrobbling_history_import_file_detail),
                    enabled = !historyBusy,
                    onClick = { importing.launch(arrayOf("*/*")) },
                    modifier = Modifier.testTag("history-import-file"),
                )
            }
            val saved = stored
            if (saved != null && saved.listenbrainzToken.isNotEmpty()) {
                item(key = "history-import-listenbrainz") {
                    SettingRow(
                        title = stringResource(R.string.scrobbling_history_import_listenbrainz),
                        icon = Icons.Rounded.CloudDownload,
                        supporting = stringResource(R.string.scrobbling_history_import_detail),
                        enabled = online && !historyBusy,
                        onClick = { model.importHistory(ScrobbleService.LISTEN_BRAINZ) },
                    )
                }
            }
            if (saved != null && saved.lastfmSession.isNotEmpty()) {
                item(key = "history-import-lastfm") {
                    SettingRow(
                        title = stringResource(R.string.scrobbling_history_import_lastfm),
                        icon = Icons.Rounded.CloudDownload,
                        supporting = stringResource(R.string.scrobbling_history_import_detail),
                        enabled = online && !historyBusy,
                        onClick = { model.importHistory(ScrobbleService.LAST_FM) },
                    )
                }
            }
            if (historyBusy || historyStatus != null) {
                item(key = "history-status") {
                    SettingRow(
                        title =
                            if (historyBusy) stringResource(R.string.scrobbling_history_working)
                            else historyStatus.orEmpty(),
                        icon = Icons.Rounded.Info,
                        modifier = Modifier.semantics { liveRegion = LiveRegionMode.Polite },
                    )
                }
            }
            item(key = "keystore") {
                Footnote(stringResource(R.string.scrobbling_keystore))
            }
        }
    }
}
