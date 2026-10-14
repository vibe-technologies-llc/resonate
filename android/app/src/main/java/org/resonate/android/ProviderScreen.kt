package org.resonate.android

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.FilterChip
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import org.resonate.android.nativecore.DownloadAccount
import org.resonate.android.nativecore.MusicProvider
import org.resonate.android.nativecore.SearchSong

@Composable
fun ProviderScreen(library: LibraryRepository) {
    val authorization by library.features.tidalAuthorization.collectAsState()
    val signingIn by library.features.tidalBusy.collectAsState()
    val uriHandler = androidx.compose.ui.platform.LocalUriHandler.current
    val saved by library.features.providerAccount.collectAsState()
    var draft by remember(saved) { mutableStateOf(saved) }
    val account = draft ?: return
    Column(
        Modifier.verticalScroll(rememberScrollState()).padding(20.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        Text("Download providers", style = MaterialTheme.typography.headlineSmall)
        Text(
            "Downloads are matched by recording codes, or by title, artist and duration. Choose the destination folder for each song.",
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            MusicProvider.entries.forEach { provider ->
                FilterChip(
                    account.provider == provider,
                    { draft = account.copy(provider = provider) },
                    label = { Text(provider.name.replace('_', ' ')) },
                )
            }
        }
        if (
            account.provider == MusicProvider.SUBSONIC ||
                account.provider == MusicProvider.MONOCHROME
        )
            OutlinedTextField(
                account.server,
                { draft = account.copy(server = it) },
                label = {
                    Text(
                        if (account.provider == MusicProvider.MONOCHROME)
                            "Server URL (blank uses the hosted service)"
                        else "Server URL"
                    )
                },
                modifier = Modifier.fillMaxWidth(),
                singleLine = true,
            )
        if (account.provider == MusicProvider.SUBSONIC) {
            OutlinedTextField(
                account.user,
                { draft = account.copy(user = it) },
                label = { Text("Username") },
                modifier = Modifier.fillMaxWidth(),
                singleLine = true,
            )
            SecretField("Password", account.password) { draft = account.copy(password = it) }
        }
        if (account.provider == MusicProvider.TIDAL) {
            OutlinedTextField(
                account.clientId,
                { draft = account.copy(clientId = it) },
                label = { Text("TIDAL client ID") },
                modifier = Modifier.fillMaxWidth(),
                singleLine = true,
            )
            SecretField("Client secret (if required)", account.clientSecret) {
                draft = account.copy(clientSecret = it)
            }
            SecretField("Refresh token", account.refreshToken) {
                draft = account.copy(refreshToken = it)
            }
            TextButton(
                onClick = { library.features.authorizeTidal(account) },
                enabled = account.clientId.isNotBlank() && !signingIn,
            ) {
                Text("Sign in with TIDAL")
            }
            authorization?.let { request ->
                Text("Enter code: ${request.userCode}", style = MaterialTheme.typography.titleLarge)
                TextButton(onClick = { uriHandler.openUri(request.verifyAt) }) {
                    Text("Open TIDAL authorization")
                }
            }
        }
        Button(onClick = { library.features.saveProviderAccount(account) }) {
            Text("Save provider")
        }
        Text(
            "Account credentials are encrypted with Android Keystore.",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

@Composable
private fun SecretField(label: String, value: String, change: (String) -> Unit) {
    OutlinedTextField(
        value,
        change,
        label = { Text(label) },
        modifier = Modifier.fillMaxWidth(),
        singleLine = true,
        visualTransformation = PasswordVisualTransformation(),
    )
}

@Composable
fun DownloadFolderDialog(library: LibraryRepository, song: SearchSong, dismiss: () -> Unit) {
    val folders by library.features.folders.collectAsState()
    val destinations = folders.filter { it.readable && it.writable }
    AlertDialog(
        onDismissRequest = dismiss,
        title = { Text("Download ${song.title}") },
        text = {
            Column {
                Text("Choose a music folder")
                if (destinations.isEmpty()) Text("Add a music folder with write access first.")
                destinations.forEach { folder ->
                    TextButton(
                        onClick = {
                            library.features.download(song, folder)
                            dismiss()
                        }
                    ) {
                        Text(folder.name)
                    }
                }
            }
        },
        confirmButton = { TextButton(onClick = dismiss) { Text("Cancel") } },
    )
}
