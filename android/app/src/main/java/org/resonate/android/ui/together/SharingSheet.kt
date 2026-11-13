package org.resonate.android.ui.together

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.Logout
import androidx.compose.material.icons.rounded.ErrorOutline
import androidx.compose.material.icons.rounded.GroupAdd
import androidx.compose.material.icons.rounded.Person
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.testTagsAsResourceId
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import kotlinx.coroutines.delay
import org.resonate.android.R
import org.resonate.android.SharingBlock
import org.resonate.android.TogetherWords
import org.resonate.android.nativecore.SharedPlaylist
import org.resonate.android.nativecore.TogetherPerson
import org.resonate.android.ui.LocalResonate
import org.resonate.android.ui.components.SectionHeader
import org.resonate.android.ui.components.Sizes
import org.resonate.android.ui.navigation.LocalNavigator
import org.resonate.android.ui.navigation.Screen
import org.resonate.android.ui.settings.ButtonRow
import org.resonate.android.ui.settings.Note
import org.resonate.android.ui.settings.NoticeCard
import org.resonate.android.ui.settings.SettingRow
import org.resonate.android.words

private const val SHEET_READ_EVERY_MS = 1_000L
private const val RESET_KEY = "reset"
private const val LEAVE_KEY = "leave"

private fun TogetherPerson.key(): String = id.joinToString("")

@Composable
fun SharingSheet(id: ULong, name: String, sharedHere: Boolean, onDismiss: () -> Unit) {
    val resonate = LocalResonate.current
    val together = resonate.features.together
    val lists by together.lists.collectAsStateWithLifecycle()
    val config by resonate.features.syncConfig.collectAsStateWithLifecycle()
    val online by resonate.features.online.collectAsStateWithLifecycle()
    val list = lists.firstOrNull { it.playlist == id }
    val words = words()
    val held = list != null
    LaunchedEffect(id) {
        while (true) {
            together.readLists()
            delay(SHEET_READ_EVERY_MS)
        }
    }
    DisposableEffect(id, held) {
        together.watch(if (held) id else null)
        onDispose { together.watch(null) }
    }
    ModalBottomSheet(onDismissRequest = onDismiss) {
        Column(
            Modifier.verticalScroll(rememberScrollState())
                .navigationBarsPadding()
                .semantics { testTagsAsResourceId = true }
                .testTag("sharing-sheet")
        ) {
            Text(
                name,
                style = MaterialTheme.typography.titleLarge,
                modifier = Modifier.padding(horizontal = Sizes.gutter, vertical = 8.dp),
            )
            if (list == null) {
                val block =
                    config?.let { TogetherWords.sharingBlock(it.enabled, it.nearbyOnly, online) }
                if (sharedHere) SharedWhileStopped(block, onDismiss)
                else NotShared(block, onDismiss) { together.share(id) }
            } else {
                Shared(id, list, onDismiss)
            }
        }
    }
}

@Composable
private fun SharingBlockNote(block: SharingBlock, onDismiss: () -> Unit) {
    val navigator = LocalNavigator.current
    val destination = if (block == SharingBlock.ONLINE_OFF) Screen.Settings else Screen.Sync
    val button =
        if (block == SharingBlock.ONLINE_OFF) R.string.sharing_open_settings
        else R.string.sharing_open_devices
    Note(TogetherWords.sharingNeeds(words(), block), Modifier.testTag("sharing-needs-sync"))
    ButtonRow {
        FilledTonalButton(
            onClick = {
                onDismiss()
                navigator.open(destination)
            }
        ) {
            Text(stringResource(button))
        }
    }
}

@Composable
private fun SharedWhileStopped(block: SharingBlock?, onDismiss: () -> Unit) {
    Note(stringResource(R.string.sharing_shared_while_stopped), Modifier.testTag("sharing-shared-stopped"))
    if (block != null) SharingBlockNote(block, onDismiss)
}

@Composable
private fun NotShared(block: SharingBlock?, onDismiss: () -> Unit, onShare: () -> Unit) {
    if (block != null) {
        SharingBlockNote(block, onDismiss)
        return
    }
    Note(stringResource(R.string.sharing_not_shared_yet))
    ButtonRow {
        FilledTonalButton(onClick = onShare, modifier = Modifier.testTag("share-playlist")) {
            Text(stringResource(R.string.sharing_share_this_playlist))
        }
    }
}

@Composable
private fun Shared(id: ULong, list: SharedPlaylist, onDismiss: () -> Unit) {
    val together = LocalResonate.current.features.together
    val press = rememberSecondPress()
    val words = words()
    TogetherWords.reach(words, list.reach)?.let { said ->
        NoticeCard(
            Icons.Rounded.ErrorOutline,
            said,
            Modifier.semantics { liveRegion = LiveRegionMode.Polite }.testTag("sharing-reach"),
        )
    }
    if (!list.mayEdit) Note(stringResource(R.string.sharing_following))
    list.editLink?.let { link ->
        InvitationLink(
            stringResource(R.string.sharing_edit_link_says),
            link,
            stringResource(R.string.sharing_edit_link_title, list.name),
            "edit",
        )
    }
    InvitationLink(
        stringResource(R.string.sharing_follow_link_says),
        list.followLink,
        stringResource(R.string.sharing_follow_link_title, list.name),
        "follow",
    )
    if (list.people.isNotEmpty()) {
        SectionHeader(stringResource(R.string.together_people))
        list.people.forEach { person ->
            val key = person.key()
            SettingRow(
                title = TogetherWords.person(words, person),
                icon = Icons.Rounded.Person,
                trailing =
                    if (list.mayEdit && !person.isMe) {
                        {
                            TextButton(
                                onClick = { press.press(key) { together.reset(id, person.id) } },
                                modifier = Modifier.testTag("sharing-remove"),
                            ) {
                                Text(press.label(key, stringResource(R.string.sharing_remove)))
                            }
                        }
                    } else null,
            )
        }
    }
    TogetherWords.notHeld(words, list.notHeld)?.let {
        Note(it, Modifier.testTag("sharing-not-held"))
    }
    ButtonRow {
        if (list.mayEdit) {
            TextButton(
                onClick = { press.press(RESET_KEY) { together.reset(id, null) } },
                modifier = Modifier.testTag("sharing-reset"),
            ) {
                Icon(Icons.Rounded.GroupAdd, null, Modifier.padding(end = 8.dp))
                Text(press.label(RESET_KEY, stringResource(R.string.sharing_make_new_links)))
            }
        }
        OutlinedButton(
            onClick = {
                press.press(LEAVE_KEY) {
                    together.leaveList(id)
                    onDismiss()
                }
            },
            colors =
                ButtonDefaults.outlinedButtonColors(contentColor = MaterialTheme.colorScheme.error),
            modifier = Modifier.testTag("sharing-leave"),
        ) {
            Icon(Icons.AutoMirrored.Rounded.Logout, null, Modifier.padding(end = 8.dp))
            Text(press.label(LEAVE_KEY, stringResource(R.string.sharing_leave)))
        }
    }
}
