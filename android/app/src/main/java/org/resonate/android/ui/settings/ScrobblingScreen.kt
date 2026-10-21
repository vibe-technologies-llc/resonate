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
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.lifecycle.ViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
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
    private val mutableDraft = MutableStateFlow<ScrobbleDraft?>(null)
    val draft = mutableDraft.asStateFlow()

    init {
        viewModelScope.launch {
            var previous: ScrobbleAccount? = null
            stored.collect { account ->
                if (account == null) return@collect
                val before = previous
                previous = account
                mutableDraft.update { current ->
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
    }

    fun edit(change: (ScrobbleDraft) -> ScrobbleDraft) {
        mutableDraft.update { it?.let(change) }
    }

    fun save() {
        val current = draft.value ?: return
        features.saveScrobbling(current.account.trimmed(), current.includeHistory)
        edit { it.copy(includeHistory = false) }
    }

    fun validate() {
        draft.value?.let { features.validateScrobbling(it.account.trimmed()) }
    }

    fun signIn() {
        val current = draft.value ?: return
        features.signInLastFm(current.account.trimmed(), current.user.trim(), current.password)
        edit { it.copy(password = "") }
    }

    fun signOut() = edit { it.copy(account = it.account.copy(lastfmSession = "")) }

    fun goOnline() = features.setOnline(true)
}

@Composable
fun ScrobblingScreen() {
    val model = resonateViewModel { ScrobblingViewModel(it) }
    val stored by model.stored.collectAsStateWithLifecycle()
    val draft by model.draft.collectAsStateWithLifecycle()
    val status by model.status.collectAsStateWithLifecycle()
    val online by model.online.collectAsStateWithLifecycle()
    Page("Scrobbling") { padding ->
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
                Note(
                    "Songs you listen to through are queued on this device and sent to the accounts saved here while online features are on."
                )
            }
            if (!online) {
                item(key = "offline") {
                    NoticeCard(
                        Icons.Rounded.CloudOff,
                        "Online features are off. Listens wait here until you turn them on.",
                        action = "Turn on",
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
                    label = "User token",
                    supporting = "Find it in your ListenBrainz settings",
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
                    label = "API key",
                    secret = true,
                )
            }
            item(key = "lastfm-secret") {
                Field(
                    value = account.lastfmSecret,
                    onValueChange = { value ->
                        model.edit { it.copy(account = it.account.copy(lastfmSecret = value)) }
                    },
                    label = "Shared secret",
                    supporting = "Both come from an API account you create at Last.fm",
                    secret = true,
                )
            }
            if (account.lastfmSession.isNotEmpty()) {
                item(key = "lastfm-session") {
                    SettingRow(
                        title = "Signed in to Last.fm",
                        icon = Icons.Rounded.CheckCircle,
                        supporting = "Resonate keeps the session, never your password",
                        trailing = { TextButton(onClick = model::signOut) { Text("Sign out") } },
                    )
                }
            } else {
                item(key = "lastfm-signed-out") {
                    SettingRow(
                        title = "Sign in to Last.fm",
                        icon = Icons.Rounded.Lock,
                        supporting =
                            "Your password is used once to start a session and is never saved",
                    )
                }
                item(key = "lastfm-user") {
                    Field(
                        value = current.user,
                        onValueChange = { value -> model.edit { it.copy(user = value) } },
                        label = "Last.fm username",
                    )
                }
                item(key = "lastfm-password") {
                    Field(
                        value = current.password,
                        onValueChange = { value -> model.edit { it.copy(password = value) } },
                        label = "Last.fm password",
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
                            Text("Sign in")
                        }
                    }
                }
            }
            item(key = "options-header") { SectionHeader("When saving") }
            item(key = "history") {
                SwitchRow(
                    title = "Include existing history",
                    icon = Icons.Rounded.History,
                    supporting = "Also send what you listened to before these accounts were saved",
                    checked = current.includeHistory,
                    onCheckedChange = { value -> model.edit { it.copy(includeHistory = value) } },
                )
            }
            item(key = "actions") {
                ButtonRow {
                    OutlinedButton(onClick = model::validate, enabled = online) {
                        Text("Check accounts")
                    }
                    Button(
                        onClick = model::save,
                        enabled = unsaved,
                        modifier = Modifier.testTag("save-scrobbling"),
                    ) {
                        Text(if (unsaved) "Save" else "Saved")
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
                    title = "Favourites are told for you",
                    icon = Icons.Rounded.Favorite,
                    supporting =
                        "A favourite with a recording is loved on each saved account, and taken back there when you remove it. There is nothing to send by hand.",
                )
            }
            item(key = "keystore") {
                Footnote("Tokens, keys and sessions stay encrypted with Android Keystore.")
            }
        }
    }
}
