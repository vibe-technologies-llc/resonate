package org.resonate.android.ui.together

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.Logout
import androidx.compose.material.icons.rounded.ErrorOutline
import androidx.compose.material.icons.rounded.Groups
import androidx.compose.material.icons.rounded.MusicNote
import androidx.compose.material.icons.rounded.Person
import androidx.compose.material.icons.rounded.Sync
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.ViewModel
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.repeatOnLifecycle
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import org.resonate.android.R
import org.resonate.android.TogetherWords
import org.resonate.android.nativecore.ListeningParty
import org.resonate.android.ui.Resonate
import org.resonate.android.ui.components.EmptyState
import org.resonate.android.ui.components.Page
import org.resonate.android.ui.components.SectionHeader
import org.resonate.android.ui.resonateViewModel
import org.resonate.android.ui.settings.ButtonRow
import org.resonate.android.ui.settings.Footnote
import org.resonate.android.ui.settings.Note
import org.resonate.android.ui.settings.NoticeCard
import org.resonate.android.ui.settings.SettingRow
import org.resonate.android.ui.settings.SettingsList
import org.resonate.android.words

const val PARTY_READ_EVERY_MS = 500L
private const val END_KEY = "end"

class PartyViewModel(private val resonate: Resonate) : ViewModel() {
    val together = resonate.features.together
    val party = together.party

    suspend fun follow() {
        while (currentCoroutineContext().isActive) {
            resonate.read { together.readParty() }
            delay(PARTY_READ_EVERY_MS)
        }
    }
}

@Composable
fun PartyScreen() {
    val model = resonateViewModel { PartyViewModel(it) }
    val party by model.party.collectAsStateWithLifecycle()
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    LaunchedEffect(lifecycle) {
        lifecycle.repeatOnLifecycle(Lifecycle.State.STARTED) { model.follow() }
    }
    Page(stringResource(R.string.party_title)) { padding ->
        val shown = party
        if (shown == null) {
            Column(
                Modifier.fillMaxSize().padding(padding).verticalScroll(rememberScrollState()),
                verticalArrangement = Arrangement.Center,
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                NoParty(model)
            }
        } else {
            SettingsList(padding, Modifier.testTag("party-list")) {
                item(key = "party") { AtTheParty(shown, model) }
            }
        }
    }
}

@Composable
private fun NoParty(model: PartyViewModel) {
    val paste = rememberPasteALink()
    EmptyState(
        Icons.Rounded.Groups,
        stringResource(R.string.party_listen_together),
        Modifier.testTag("party-empty"),
        detail = stringResource(R.string.party_no_party_yet),
        action = stringResource(R.string.party_begin),
        onAction = model.together::beginParty,
        second = {
            TextButton(onClick = paste, modifier = Modifier.testTag("party-paste")) {
                Text(stringResource(R.string.party_paste_a_link))
            }
        },
    )
}

@Composable
private fun AtTheParty(party: ListeningParty, model: PartyViewModel) {
    val together = model.together
    val press = rememberSecondPress()
    val over = TogetherWords.isOver(party.connection)
    val words = words()
    Column {
        TogetherWords.connection(words, party.connection)?.let { said ->
            NoticeCard(
                if (over) Icons.Rounded.ErrorOutline else Icons.Rounded.Sync,
                said,
                Modifier.semantics { liveRegion = LiveRegionMode.Polite }.testTag("party-connection"),
                error = over,
            )
        }
        if (over) {
            ButtonRow {
                Button(onClick = together::leaveParty, modifier = Modifier.testTag("party-close")) {
                    Text(stringResource(R.string.party_close))
                }
            }
            return@Column
        }
        party.song?.let { song ->
            SectionHeader(stringResource(R.string.party_playing_now))
            SettingRow(
                title = song.title,
                icon = Icons.Rounded.MusicNote,
                supporting = song.artist.ifBlank { null },
                modifier = Modifier.testTag("party-song"),
            )
        }
        if (party.hosting) {
            SectionHeader(stringResource(R.string.party_invite_people))
            InvitationLink(
                stringResource(R.string.party_link_says),
                party.link,
                stringResource(R.string.party_link_title),
                "party",
                qrAtFirst = true,
            )
        } else {
            Note(
                TogetherWords.following(words, party.following),
                Modifier.semantics { liveRegion = LiveRegionMode.Polite }.testTag("party-following"),
            )
        }
        if (party.people.isNotEmpty()) SectionHeader(stringResource(R.string.party_here))
        party.people.forEach { person ->
            SettingRow(
                title = TogetherWords.person(words, person),
                icon = Icons.Rounded.Person,
                supporting = if (person.isHost) stringResource(R.string.party_host) else null,
                trailing =
                    if (party.hosting && !person.isMe) {
                        {
                            TextButton(
                                onClick = { together.turnOut(person.id) },
                                modifier = Modifier.testTag("party-remove"),
                            ) {
                                Text(stringResource(R.string.party_remove))
                            }
                        }
                    } else null,
                modifier = Modifier.testTag("party-person"),
            )
        }
        if (party.hosting && party.requests.isNotEmpty()) {
            SectionHeader(stringResource(R.string.party_asked_for))
            party.requests.forEach { request ->
                SettingRow(
                    title = request.title,
                    icon = Icons.Rounded.MusicNote,
                    supporting = request.artist.ifBlank { null },
                    modifier = Modifier.testTag("party-request"),
                )
                ButtonRow {
                    TextButton(onClick = { together.passOver(request.id) }) {
                        Text(stringResource(R.string.party_pass))
                    }
                    OutlinedButton(onClick = { together.playNext(request.id) }) {
                        Text(stringResource(R.string.party_play_next))
                    }
                }
            }
        }
        if (!party.hosting) Note(stringResource(R.string.party_asks_the_host))
        ButtonRow {
            OutlinedButton(
                onClick = {
                    if (party.hosting) press.press(END_KEY) { together.leaveParty() }
                    else together.leaveParty()
                },
                colors =
                    ButtonDefaults.outlinedButtonColors(
                        contentColor = MaterialTheme.colorScheme.error
                    ),
                modifier = Modifier.testTag("party-leave"),
            ) {
                Icon(Icons.AutoMirrored.Rounded.Logout, null, Modifier.padding(end = 8.dp))
                Text(
                    if (party.hosting)
                        press.label(END_KEY, stringResource(R.string.party_notification_end))
                    else stringResource(R.string.party_notification_leave)
                )
            }
        }
        party.roundTripMs?.let { Footnote(TogetherWords.roundTrip(words, it)) }
    }
}
