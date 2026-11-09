package org.resonate.android.ui.settings

import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.CheckCircle
import androidx.compose.material.icons.rounded.CloudOff
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
import kotlinx.coroutines.launch
import org.resonate.android.R
import org.resonate.android.nativecore.ScrobbleAccount
import org.resonate.android.ui.Resonate
import org.resonate.android.ui.components.Page
import org.resonate.android.ui.components.SectionHeader
import org.resonate.android.ui.resonateViewModel

data class ScrobbleDraft(
    val account: ScrobbleAccount,
    val includeHistory: Boolean = false,
    val user: String = "",
    val password: String = "",
)

fun ScrobbleAccount.trimmed(): ScrobbleAccount =
    copy(
        listenbrainzToken = listenbrainzToken.trim(),
        lastfmKey = lastfmKey.trim(),
        lastfmSecret = lastfmSecret.trim(),
    )

fun canSignInToLastFm(draft: ScrobbleDraft): Boolean =
    draft.account.lastfmKey.isNotBlank() &&
        draft.account.lastfmSecret.isNotBlank() &&
        draft.user.isNotBlank() &&
        draft.password.isNotEmpty()

class ScrobblingViewModel(resonate: Resonate) : ViewModel() {
    private val features = resonate.features

    val stored = features.scrobbleAccount
    val status = features.scrobbleStatus
    val online = features.online
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
        features.signInLastFm(current.account.trimmed(), current.user.trim(), current.password) {
            viewModelScope.launch { edit { it.copy(password = "") } }
        }
    }

    fun signOut() = features.signOutLastFm()

    fun goOnline() = features.setOnline(true)
}

@Composable
fun ScrobblingScreen() {
    val model = resonateViewModel { ScrobblingViewModel(it) }
    val stored by model.stored.collectAsStateWithLifecycle()
    val draft = model.draft
    val status by model.status.collectAsStateWithLifecycle()
    val online by model.online.collectAsStateWithLifecycle()
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
                        supporting = stringResource(R.string.scrobbling_lastfm_sign_in_detail),
                    )
                }
                item(key = "lastfm-user") {
                    Field(
                        value = current.user,
                        onValueChange = { value -> model.edit { it.copy(user = value) } },
                        label = stringResource(R.string.scrobbling_lastfm_user),
                    )
                }
                item(key = "lastfm-password") {
                    Field(
                        value = current.password,
                        onValueChange = { value -> model.edit { it.copy(password = value) } },
                        label = stringResource(R.string.scrobbling_lastfm_password),
                        secret = true,
                    )
                }
                item(key = "lastfm-sign-in") {
                    ButtonRow {
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
            item(key = "keystore") {
                Footnote(stringResource(R.string.scrobbling_keystore))
            }
        }
    }
}
