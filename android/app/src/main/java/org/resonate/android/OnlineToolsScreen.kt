package org.resonate.android

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp

@Composable
fun AutoEqScreen(library: LibraryRepository) {
    val devices by library.features.corrections.collectAsState()
    val busy by library.features.correctionBusy.collectAsState()
    val online by library.features.online.collectAsState()
    var query by remember { mutableStateOf("") }
    Column(
        Modifier.fillMaxSize().padding(20.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        Text(
            "AutoEq headphone correction",
            style = MaterialTheme.typography.headlineSmall,
            fontWeight = FontWeight.Bold,
        )
        OutlinedTextField(
            query,
            { query = it },
            label = { Text("Headphone model") },
            modifier = Modifier.fillMaxWidth(),
            shape = RoundedCornerShape(16.dp),
            singleLine = true,
        )
        Button(
            onClick = { library.features.searchCorrections(query) },
            enabled = online && query.isNotBlank() && !busy,
            shape = RoundedCornerShape(16.dp),
        ) {
            Text("Find correction")
        }
        if (!online) Text("Enable online features in Settings to fetch corrections.")
        if (busy) LinearProgressIndicator(Modifier.fillMaxWidth())
        Text(
            "A selected profile applies to every route. Switch it off or edit the ten-band curve in Equaliser settings.",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        LazyColumn(verticalArrangement = Arrangement.spacedBy(10.dp)) {
            items(devices, key = { it.id }) { device ->
                Surface(
                    shape = RoundedCornerShape(18.dp),
                    color = MaterialTheme.colorScheme.surfaceContainerLow,
                    border = BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.25f)),
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Row(
                        Modifier.fillMaxWidth().padding(16.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.SpaceBetween,
                    ) {
                        Text(
                            device.name,
                            Modifier.weight(1f),
                            style = MaterialTheme.typography.titleMedium,
                            fontWeight = FontWeight.SemiBold,
                        )
                        TextButton(
                            onClick = { library.features.applyCorrection(device) },
                            enabled = !busy,
                        ) {
                            Text("Apply correction")
                        }
                    }
                }
            }
        }
    }
}

@Composable
fun ScrobblingScreen(library: LibraryRepository) {
    val saved by library.features.scrobbleAccount.collectAsState()
    val status by library.features.scrobbleStatus.collectAsState()
    var draft by remember(saved) { mutableStateOf(saved) }
    var history by remember { mutableStateOf(false) }
    var username by remember { mutableStateOf("") }
    var password by remember { mutableStateOf("") }
    val account = draft ?: return
    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(20.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        Text(
            "Scrobbling",
            style = MaterialTheme.typography.headlineSmall,
            fontWeight = FontWeight.Bold,
        )
        Text(
            "Completed listens are queued on this device and submitted when online features are enabled.",
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            style = MaterialTheme.typography.bodyMedium,
        )
        Surface(
            shape = RoundedCornerShape(24.dp),
            color = MaterialTheme.colorScheme.surfaceContainerLow,
            border = BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.25f)),
            modifier = Modifier.fillMaxWidth(),
        ) {
            Column(Modifier.padding(18.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                SecretInput("ListenBrainz token", account.listenbrainzToken) {
                    draft = account.copy(listenbrainzToken = it)
                }
                SecretInput("Last.fm application key", account.lastfmKey) {
                    draft = account.copy(lastfmKey = it)
                }
                SecretInput("Last.fm application secret", account.lastfmSecret) {
                    draft = account.copy(lastfmSecret = it)
                }
                SecretInput("Last.fm session", account.lastfmSession) {
                    draft = account.copy(lastfmSession = it)
                }
                OutlinedTextField(
                    username,
                    { username = it },
                    label = { Text("Last.fm username") },
                    modifier = Modifier.fillMaxWidth(),
                    shape = RoundedCornerShape(16.dp),
                    singleLine = true,
                )
                SecretInput("Last.fm password (used only to sign in)", password) { password = it }
                TextButton(
                    onClick = {
                        library.features.signInLastFm(account, username, password)
                        password = ""
                    },
                    enabled =
                        username.isNotBlank() &&
                            password.isNotBlank() &&
                            account.lastfmKey.isNotBlank() &&
                            account.lastfmSecret.isNotBlank(),
                ) {
                    Text("Sign in to Last.fm")
                }
            }
        }
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text("Include existing listening history", Modifier.weight(1f))
            Switch(history, { history = it })
        }
        Button(
            onClick = {
                library.features.saveScrobbling(account, history)
                history = false
            },
            shape = RoundedCornerShape(16.dp),
        ) {
            Text("Save scrobbling accounts")
        }
        TextButton(onClick = { library.features.validateScrobbling(account) }) {
            Text("Validate accounts")
        }
        TextButton(onClick = library.features::sendFavourites) {
            Text("Send current favourites to saved accounts")
        }
        Text(
            "Sends favourites with identified recordings. Removing a favourite locally does not remove it from your online accounts.",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        status?.let { Text(it, color = MaterialTheme.colorScheme.primary) }
    }
}

@Composable
private fun SecretInput(label: String, value: String, change: (String) -> Unit) {
    OutlinedTextField(
        value,
        change,
        label = { Text(label) },
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(16.dp),
        singleLine = true,
        visualTransformation = PasswordVisualTransformation(),
    )
}
