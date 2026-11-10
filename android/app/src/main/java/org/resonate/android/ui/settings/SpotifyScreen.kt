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
import androidx.compose.ui.res.stringResource
import androidx.lifecycle.ViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import org.resonate.android.R
import org.resonate.android.SpotifyWords
import org.resonate.android.nativecore.spotifyAccountLink
import org.resonate.android.nativecore.spotifyRedirect
import org.resonate.android.ui.Resonate
import org.resonate.android.ui.components.Page
import org.resonate.android.ui.components.SectionHeader
import org.resonate.android.ui.resonateViewModel
import org.resonate.android.words

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
    val words = words()
    Page("Spotify") { padding ->
        SettingsList(padding, Modifier.testTag("spotify-list")) {
            if (!online) {
                item(key = "offline") {
                    NoticeCard(
                        Icons.Rounded.CloudOff,
                        stringResource(R.string.spotify_screen_offline),
                        action = stringResource(R.string.spotify_screen_turn_on),
                        onAction = model::goOnline,
                    )
                }
            }
            item(key = "app-header") {
                SectionHeader(stringResource(R.string.spotify_screen_your_app))
            }
            item(key = "client") {
                Field(
                    value = model.clientId,
                    onValueChange = model::type,
                    label = stringResource(R.string.spotify_screen_client_id),
                    supporting = SpotifyWords.register(words, spotifyRedirect()),
                    modifier = Modifier.testTag("spotify-client"),
                )
            }
            if (account?.clientId != model.clientId.trim()) {
                item(key = "keep-client") {
                    ButtonRow {
                        OutlinedButton(onClick = model::keepClient) {
                            Text(stringResource(R.string.spotify_screen_save))
                        }
                    }
                }
            }
            item(key = "account-header") {
                SectionHeader(stringResource(R.string.spotify_screen_your_account))
            }
            if (account?.signedIn == true) {
                item(key = "signed-in") {
                    SettingRow(
                        title = stringResource(R.string.spotify_screen_signed_in),
                        icon = Icons.Rounded.CheckCircle,
                        supporting = stringResource(R.string.spotify_screen_signed_in_detail),
                        trailing = {
                            TextButton(onClick = model::signOut) {
                                Text(stringResource(R.string.spotify_screen_sign_out))
                            }
                        },
                    )
                }
                item(key = "actions") {
                    ButtonRow {
                        Button(
                            onClick = model::follow,
                            enabled = online && !reading,
                            modifier = Modifier.testTag("spotify-follow"),
                        ) {
                            Text(
                                stringResource(
                                    if (reading) R.string.spotify_screen_reading
                                    else R.string.spotify_screen_follow
                                )
                            )
                        }
                        OutlinedButton(onClick = model::moveOnce, enabled = online && !reading) {
                            Text(stringResource(R.string.spotify_screen_move_once))
                        }
                    }
                }
                item(key = "what-it-does") {
                    Footnote(stringResource(R.string.spotify_words_what_following_does))
                }
            } else if (signingIn) {
                item(key = "waiting") {
                    SettingRow(
                        title = stringResource(R.string.spotify_screen_waiting),
                        icon = Icons.Rounded.Link,
                        supporting = stringResource(R.string.spotify_screen_waiting_detail),
                    )
                }
                item(key = "waiting-actions") {
                    ButtonRow {
                        address?.let { page ->
                            OutlinedButton(onClick = { model.open(page) }) {
                                Text(stringResource(R.string.spotify_screen_open_page))
                            }
                        }
                        TextButton(onClick = model::cancelSignIn) {
                            Text(stringResource(R.string.spotify_screen_cancel))
                        }
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
                            Text(stringResource(R.string.spotify_screen_sign_in))
                        }
                    }
                }
            }
            item(key = "limits") {
                Footnote(stringResource(R.string.spotify_words_what_spotify_allows))
            }
        }
    }
}
