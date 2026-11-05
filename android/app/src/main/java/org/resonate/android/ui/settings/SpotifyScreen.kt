package org.resonate.android.ui.settings

import android.net.Uri
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.CheckCircle
import androidx.compose.material.icons.rounded.CloudOff
import androidx.compose.material.icons.rounded.Link
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
import androidx.lifecycle.ViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import org.resonate.android.SpotifyWords
import org.resonate.android.nativecore.spotifyAccountLink
import org.resonate.android.nativecore.spotifyRedirect
import org.resonate.android.ui.Resonate
import org.resonate.android.ui.components.Page
import org.resonate.android.ui.components.SectionHeader
import org.resonate.android.ui.resonateViewModel

class SpotifyViewModel(private val resonate: Resonate) : ViewModel() {
    private val features = resonate.features

    val account = features.spotifyAccount
    val address = features.spotifyAddress
    val signingIn = features.spotifySigningIn
    val following = features.followingProfiles
    val online = features.online
    var clientId by mutableStateOf("")
        private set

    init {
        viewModelScope.launch {
            features.spotifyAccount.first { it != null }?.let { clientId = it.clientId }
        }
    }

    fun type(value: String) {
        clientId = value
    }

    fun keepClient() = features.keepSpotifyClient(clientId)

    fun signIn() = features.signInToSpotify(clientId)

    fun cancelSignIn() = features.cancelSpotifySignIn()

    fun signOut() = features.signOutOfSpotify()

    fun follow() = features.followProfile(spotifyAccountLink(), keptInStep = true)

    fun moveOnce() = features.followProfile(spotifyAccountLink(), keptInStep = false)

    fun open(page: String) = resonate.host.open(Uri.parse(page))

    fun goOnline() = features.setOnline(true)
}

@Composable
fun SpotifyScreen() {
    val model = resonateViewModel { SpotifyViewModel(it) }
    val account by model.account.collectAsStateWithLifecycle()
    val address by model.address.collectAsStateWithLifecycle()
    val signingIn by model.signingIn.collectAsStateWithLifecycle()
    val following by model.following.collectAsStateWithLifecycle()
    val online by model.online.collectAsStateWithLifecycle()
    val reading = spotifyAccountLink() in following
    LaunchedEffect(address) { address?.let(model::open) }
    Page("Spotify") { padding ->
        SettingsList(padding, Modifier.testTag("spotify-list")) {
            if (!online) {
                item(key = "offline") {
                    NoticeCard(
                        Icons.Rounded.CloudOff,
                        "Online features are off. A Spotify account is read only while they are on.",
                        action = "Turn on",
                        onAction = model::goOnline,
                    )
                }
            }
            item(key = "app-header") { SectionHeader("Your app") }
            item(key = "client") {
                Field(
                    value = model.clientId,
                    onValueChange = model::type,
                    label = "Client ID",
                    supporting = SpotifyWords.register(spotifyRedirect()),
                    modifier = Modifier.testTag("spotify-client"),
                )
            }
            if (account?.clientId != model.clientId.trim()) {
                item(key = "keep-client") {
                    ButtonRow {
                        OutlinedButton(onClick = model::keepClient) { Text("Save") }
                    }
                }
            }
            item(key = "account-header") { SectionHeader("Your account") }
            if (account?.signedIn == true) {
                item(key = "signed-in") {
                    SettingRow(
                        title = "Signed in to Spotify",
                        icon = Icons.Rounded.CheckCircle,
                        supporting = "Resonate keeps a refresh token, encrypted, and no password",
                        trailing = { TextButton(onClick = model::signOut) { Text("Sign out") } },
                    )
                }
                item(key = "actions") {
                    ButtonRow {
                        Button(
                            onClick = model::follow,
                            enabled = online && !reading,
                            modifier = Modifier.testTag("spotify-follow"),
                        ) {
                            Text(if (reading) "Reading the account…" else "Follow the account")
                        }
                        OutlinedButton(onClick = model::moveOnce, enabled = online && !reading) {
                            Text("Move to Resonate once")
                        }
                    }
                }
                item(key = "what-it-does") { Footnote(SpotifyWords.WHAT_FOLLOWING_DOES) }
            } else if (signingIn) {
                item(key = "waiting") {
                    SettingRow(
                        title = "Waiting for Spotify",
                        icon = Icons.Rounded.Link,
                        supporting = "Approve the sign-in in your browser, then come back here",
                    )
                }
                item(key = "waiting-actions") {
                    ButtonRow {
                        address?.let { page ->
                            OutlinedButton(onClick = { model.open(page) }) { Text("Open the page") }
                        }
                        TextButton(onClick = model::cancelSignIn) { Text("Cancel") }
                    }
                }
            } else {
                item(key = "sign-in") {
                    ButtonRow {
                        FilledTonalButton(
                            onClick = model::signIn,
                            enabled = online && SpotifyWords.canSignIn(model.clientId),
                            modifier = Modifier.testTag("spotify-sign-in"),
                        ) {
                            Text("Sign in to Spotify")
                        }
                    }
                }
            }
            item(key = "limits") { Footnote(SpotifyWords.WHAT_SPOTIFY_ALLOWS) }
        }
    }
}
