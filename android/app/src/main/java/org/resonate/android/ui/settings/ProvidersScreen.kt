package org.resonate.android.ui.settings

import android.net.Uri
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.OpenInNew
import androidx.compose.material.icons.rounded.CheckCircle
import androidx.compose.material.icons.rounded.Cloud
import androidx.compose.material.icons.rounded.ContentCopy
import androidx.compose.material.icons.rounded.Dns
import androidx.compose.material.icons.rounded.Lock
import androidx.compose.material.icons.rounded.MusicNote
import androidx.compose.material.icons.rounded.Waves
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.resonate.android.nativecore.DownloadAccount
import org.resonate.android.nativecore.MusicProvider
import org.resonate.android.ui.Resonate
import org.resonate.android.ui.components.Page
import org.resonate.android.ui.components.SectionHeader
import org.resonate.android.ui.components.Sizes
import org.resonate.android.ui.resonateViewModel

class ProvidersViewModel(private val resonate: Resonate) : ViewModel() {
    private val features = resonate.features

    val stored = features.providerAccount
    val authorization = features.tidalAuthorization
    val busy = features.tidalBusy
    val hostedServices = features.hostedServices
    private val mutableDraft = MutableStateFlow<DownloadAccount?>(null)
    val draft = mutableDraft.asStateFlow()
    private var identity: String? = null

    init {
        viewModelScope.launch {
            stored.collect { account ->
                if (account == null) return@collect
                val seen = accountIdentity(account)
                if (seen != identity) {
                    identity = seen
                    mutableDraft.value = account
                }
            }
        }
    }

    fun edit(change: (DownloadAccount) -> DownloadAccount) {
        mutableDraft.update { it?.let(change) }
    }

    fun save() {
        draft.value?.let { features.saveProviderAccount(it.trimmed()) }
    }

    fun setHostedServices(enabled: Boolean) = features.setHostedServices(enabled)

    fun signIn() {
        draft.value?.let { features.authorizeTidal(it.trimmed()) }
    }

    fun cancelSignIn() =
        resonate.attempt(viewModelScope) { native ->
            withContext(Dispatchers.IO) { native.cancelTidalAuthorization() }
        }

    fun open(link: String) = resonate.host.open(Uri.parse(link))
}

private fun iconFor(provider: MusicProvider): ImageVector =
    when (provider) {
        MusicProvider.MONOCHROME -> Icons.Rounded.MusicNote
        MusicProvider.HIFI_API -> Icons.Rounded.Waves
        MusicProvider.SUBSONIC -> Icons.Rounded.Dns
        MusicProvider.TIDAL -> Icons.Rounded.Cloud
    }

private fun fieldLabel(field: ProviderField, provider: MusicProvider): String =
    when (field) {
        ProviderField.SERVER ->
            if (provider == MusicProvider.SUBSONIC) "Server address" else "Server address (optional)"
        ProviderField.USER -> "Username"
        ProviderField.PASSWORD ->
            if (provider == MusicProvider.HIFI_API) "Key for your own server (optional)" else "Password"
        ProviderField.CLIENT_ID -> "TIDAL client ID"
        ProviderField.CLIENT_SECRET -> "Client secret (if required)"
        ProviderField.REFRESH_TOKEN -> "Refresh token"
    }

private fun supportOf(field: ProviderField, provider: MusicProvider): String? =
    when (field) {
        ProviderField.SERVER ->
            if (provider == MusicProvider.SUBSONIC) "For example https://music.example.org"
            else "Leave blank to use the hosted service"
        ProviderField.REFRESH_TOKEN -> "Filled in by Sign in with TIDAL"
        else -> null
    }

private val secretFields =
    setOf(ProviderField.PASSWORD, ProviderField.CLIENT_SECRET, ProviderField.REFRESH_TOKEN)

@Composable
fun ProvidersScreen() {
    val model = resonateViewModel { ProvidersViewModel(it) }
    val stored by model.stored.collectAsStateWithLifecycle()
    val draft by model.draft.collectAsStateWithLifecycle()
    val authorization by model.authorization.collectAsStateWithLifecycle()
    val busy by model.busy.collectAsStateWithLifecycle()
    val hostedServices by model.hostedServices.collectAsStateWithLifecycle()
    val copy = rememberCopy()
    Page("Download sources") { padding ->
        val account = draft
        if (account == null) {
            Loading(padding)
            return@Page
        }
        val unsaved = stored?.let { it.trimmed() != account.trimmed() } ?: true
        SettingsList(padding, Modifier.testTag("providers-list")) {
            item(key = "intro") {
                Note(
                    "Songs you find in Search can be downloaded from the source chosen here. Downloads are matched by recording codes, or by title, artist and length, and you choose the folder for each song."
                )
            }
            item(key = "hosted-services") {
                SwitchRow(
                    title = "Ask the hosted services",
                    icon = Icons.Rounded.Cloud,
                    supporting =
                        "A Monochrome or hifi-api source with no server address is asked at the hosted service unless this is off",
                    checked = hostedServices,
                    onCheckedChange = model::setHostedServices,
                    modifier = Modifier.testTag("setting-hosted-services"),
                )
            }
            item(key = "source-header") { SectionHeader("Source") }
            item(key = "sources") {
                Column(Modifier.selectableGroup()) {
                    providersShown.forEach { provider ->
                        RadioRow(
                            title = providerName(provider),
                            supporting = providerDetail(provider),
                            icon = iconFor(provider),
                            selected = account.provider == provider,
                            onClick = { model.edit { it.copy(provider = provider) } },
                            modifier = Modifier.testTag("provider-${provider.name.lowercase()}"),
                        )
                    }
                }
            }
            item(key = "account-header") {
                SectionHeader(
                    if (account.provider == MusicProvider.TIDAL) "TIDAL account"
                    else providerName(account.provider)
                )
            }
            providerFields(account.provider).forEach { field ->
                item(key = "field-${field.name}") {
                    Field(
                        value = account.valueOf(field),
                        onValueChange = { value -> model.edit { it.with(field, value) } },
                        label = fieldLabel(field, account.provider),
                        supporting = supportOf(field, account.provider),
                        secret = field in secretFields,
                        keyboardType =
                            if (field == ProviderField.SERVER) KeyboardType.Uri else KeyboardType.Text,
                        modifier = Modifier.testTag("provider-${field.name.lowercase()}"),
                    )
                }
            }
            if (account.provider == MusicProvider.TIDAL) {
                item(key = "tidal") {
                    TidalSignIn(
                        signedIn = stored?.provider == MusicProvider.TIDAL && stored?.refreshToken?.isNotEmpty() == true,
                        busy = busy,
                        code = authorization?.userCode?.takeIf { busy },
                        onSignIn = model::signIn,
                        onOpen = { authorization?.verifyAt?.let(model::open) },
                        onCopy = { authorization?.userCode?.let { copy("TIDAL code", it) } },
                        onCancel = { model.cancelSignIn() },
                    )
                }
            }
            item(key = "save") {
                ButtonRow {
                    Button(
                        onClick = model::save,
                        enabled = unsaved,
                        modifier = Modifier.testTag("save-provider"),
                    ) {
                        Text(if (unsaved) "Save" else "Saved")
                    }
                }
            }
            item(key = "keystore") {
                Footnote("Passwords, secrets and tokens stay encrypted with Android Keystore.")
            }
        }
    }
}

@Composable
private fun TidalSignIn(
    signedIn: Boolean,
    busy: Boolean,
    code: String?,
    onSignIn: () -> Unit,
    onOpen: () -> Unit,
    onCopy: () -> Unit,
    onCancel: () -> Unit,
) {
    SettingRow(
        title = if (signedIn) "Signed in to TIDAL" else "Not signed in",
        icon = if (signedIn) Icons.Rounded.CheckCircle else Icons.Rounded.Lock,
        supporting =
            if (signedIn) "Renewed sign-ins are saved as they arrive"
            else "Sign in on the web with a code; your password never passes through Resonate",
        trailing = if (busy && code == null) ({ SmallLoading() }) else null,
    )
    if (!busy) {
        ButtonRow {
            FilledTonalButton(onClick = onSignIn, modifier = Modifier.testTag("tidal-sign-in")) {
                Text(if (signedIn) "Sign in again" else "Sign in with TIDAL")
            }
        }
    }
    if (code != null) {
        Card(
            modifier =
                Modifier.fillMaxWidth()
                    .padding(horizontal = Sizes.gutter, vertical = 8.dp)
                    .semantics { liveRegion = LiveRegionMode.Polite },
            shape = MaterialTheme.shapes.extraLarge,
            colors =
                CardDefaults.cardColors(
                    containerColor = MaterialTheme.colorScheme.secondaryContainer,
                    contentColor = MaterialTheme.colorScheme.onSecondaryContainer,
                ),
        ) {
            Column(Modifier.padding(vertical = 16.dp)) {
                Row(
                    Modifier.padding(horizontal = 16.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    SmallLoading()
                    Spacer(Modifier.width(12.dp))
                    Text(
                        "Open TIDAL and enter this code. This page carries on once you approve.",
                        style = MaterialTheme.typography.bodyMedium,
                    )
                }
                CodeText(code)
                ButtonRow {
                    TextButton(onClick = onCancel) { Text("Cancel") }
                    OutlinedButton(onClick = onCopy) {
                        Icon(Icons.Rounded.ContentCopy, contentDescription = null)
                        Spacer(Modifier.width(8.dp))
                        Text("Copy")
                    }
                    Button(onClick = onOpen) {
                        Icon(Icons.AutoMirrored.Rounded.OpenInNew, contentDescription = null)
                        Spacer(Modifier.width(8.dp))
                        Text("Open TIDAL")
                    }
                }
            }
        }
    }
}
