package org.resonate.android.ui.components

import android.graphics.BitmapFactory
import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.CloudOff
import androidx.compose.material.icons.rounded.Hearing
import androidx.compose.material.icons.rounded.MicOff
import androidx.compose.material.icons.rounded.MusicNote
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.resonate.android.ListenFollowing
import org.resonate.android.ListenPlan
import org.resonate.android.ListenStage
import org.resonate.android.nativecore.HeardSong
import org.resonate.android.ui.LocalResonate
import org.resonate.android.ui.navigation.LocalNavigator
import org.resonate.android.ui.navigation.Screen

@Composable
fun ListenSheet(dismiss: () -> Unit) {
    val resonate = LocalResonate.current
    val listening = resonate.features.listening
    val stage by listening.stage.collectAsStateWithLifecycle()
    val heard by listening.heard.collectAsStateWithLifecycle()
    val keeps by listening.keepsListening.collectAsStateWithLifecycle()
    val following by listening.following.collectAsStateWithLifecycle()
    LaunchedEffect(Unit) { resonate.listen() }
    DisposableEffect(Unit) { onDispose { listening.stop() } }
    ModalBottomSheet(onDismissRequest = dismiss) {
        Column(
            Modifier.verticalScroll(rememberScrollState())
                .navigationBarsPadding()
                .padding(horizontal = 24.dp)
                .testTag("listen-sheet"),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Column(
                Modifier.fillMaxWidth().semantics { liveRegion = LiveRegionMode.Polite },
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Stage(stage, dismiss)
            }
            if (stage is ListenStage.Found || stage == ListenStage.Unknown) {
                Spacer(Modifier.height(8.dp))
                Row(
                    Modifier.fillMaxWidth()
                        .toggleable(
                            value = keeps,
                            role = Role.Switch,
                            onValueChange = resonate::keepListening,
                        )
                        .testTag("keep-listening"),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.SpaceBetween,
                ) {
                    Column(Modifier.weight(1f)) {
                        Text("Keep listening", style = MaterialTheme.typography.titleMedium)
                        Text(
                            when (following) {
                                ListenFollowing.OFF -> "Name the next song on its own"
                                ListenFollowing.WAITING -> "Listening again in a moment"
                                ListenFollowing.LISTENING -> "Listening now"
                            },
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                    Switch(checked = keeps, onCheckedChange = null)
                }
            }
            if (heard.size > 1 || (heard.isNotEmpty() && stage !is ListenStage.Found)) {
                HorizontalDivider(Modifier.padding(vertical = 12.dp))
                Text(
                    "Heard earlier",
                    style = MaterialTheme.typography.titleSmall,
                    modifier = Modifier.fillMaxWidth(),
                )
                heard.filter { (stage as? ListenStage.Found)?.song?.key != it.key }.forEach { song ->
                    ListItem(
                        colors = ListItemDefaults.colors(containerColor = Color.Transparent),
                        supportingContent = { Text(song.artist, maxLines = 1, overflow = TextOverflow.Ellipsis) },
                    ) {
                        Text(song.title, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    }
                }
            }
            Spacer(Modifier.height(16.dp))
        }
    }
}

@Composable
private fun Stage(stage: ListenStage, dismiss: () -> Unit) {
    val resonate = LocalResonate.current
    when (stage) {
        ListenStage.Idle ->
            Message(Icons.Rounded.Hearing, "Ready to listen", null) {
                Button(onClick = resonate::listen) { Text("Listen") }
            }
        is ListenStage.Recording ->
            Working(
                "Listening…",
                if (stage.nth > 1) "Try ${stage.nth} of ${ListenPlan.TRIES_ON_A_MISS}" else "Hold the phone near the music",
            )
        ListenStage.Asking -> Working("Asking what it is…", "Shazam first, then AudD where it has a token")
        is ListenStage.Found -> Found(stage.song, dismiss)
        ListenStage.Unknown ->
            Message(Icons.Rounded.MusicNote, "Not recognised", "Nothing known matches what was heard.") {
                Button(onClick = resonate::listen) { Text("Listen again") }
            }
        ListenStage.Silent ->
            Message(Icons.Rounded.MicOff, "Nothing heard", "Move closer to the sound, or turn it up.") {
                Button(onClick = resonate::listen) { Text("Listen again") }
            }
        is ListenStage.Unreached ->
            Message(Icons.Rounded.CloudOff, "${stage.service} could not be reached", "Check the connection and try again.") {
                Button(onClick = resonate::listen) { Text("Listen again") }
            }
        ListenStage.Offline ->
            Message(Icons.Rounded.CloudOff, "Online features are off", "Naming a song asks a recognition service.") {
                Button(onClick = { resonate.features.setOnline(true); resonate.listen() }) { Text("Turn on and listen") }
            }
        ListenStage.Denied ->
            Message(Icons.Rounded.MicOff, "Resonate needs the microphone", "Allow it in Settings to name songs.") {
                Button(onClick = resonate.host::openAppSettings) { Text("Open Settings") }
            }
        is ListenStage.Failed ->
            Message(Icons.Rounded.MicOff, "Could not listen", stage.text) {
                Button(onClick = resonate::listen) { Text("Try again") }
            }
    }
}

@Composable
private fun Working(title: String, detail: String) {
    Spacer(Modifier.height(24.dp))
    CircularProgressIndicator(Modifier.testTag("listening"))
    Spacer(Modifier.height(16.dp))
    Text(title, style = MaterialTheme.typography.titleLarge)
    Text(detail, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
    Spacer(Modifier.height(24.dp))
}

@Composable
private fun Message(icon: ImageVector, title: String, detail: String?, action: @Composable () -> Unit) {
    Spacer(Modifier.height(24.dp))
    Icon(icon, contentDescription = null, Modifier.size(40.dp), tint = MaterialTheme.colorScheme.primary)
    Spacer(Modifier.height(12.dp))
    Text(title, style = MaterialTheme.typography.titleLarge, textAlign = TextAlign.Center)
    if (detail != null) {
        Text(
            detail,
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            textAlign = TextAlign.Center,
        )
    }
    Spacer(Modifier.height(16.dp))
    action()
    Spacer(Modifier.height(8.dp))
}

@Composable
private fun Found(song: HeardSong, dismiss: () -> Unit) {
    val resonate = LocalResonate.current
    val navigator = LocalNavigator.current
    val cover = remember(song.key, song.picture) {
        song.picture?.let { BitmapFactory.decodeByteArray(it, 0, it.size) }?.asImageBitmap()
    }
    Spacer(Modifier.height(16.dp))
    if (cover != null) {
        Image(
            cover,
            contentDescription = null,
            contentScale = ContentScale.Crop,
            modifier = Modifier.size(160.dp).clip(RoundedCornerShape(16.dp)),
        )
    } else {
        Icon(Icons.Rounded.MusicNote, null, Modifier.size(64.dp), tint = MaterialTheme.colorScheme.primary)
    }
    Spacer(Modifier.height(12.dp))
    Text(
        song.title,
        style = MaterialTheme.typography.titleLarge,
        textAlign = TextAlign.Center,
        maxLines = 2,
        overflow = TextOverflow.Ellipsis,
        modifier = Modifier.testTag("heard-title"),
    )
    Text(song.artist, style = MaterialTheme.typography.titleMedium, textAlign = TextAlign.Center)
    val detail = listOfNotNull(song.album.takeIf { it.isNotBlank() }, song.year?.toString()).joinToString(" · ")
    if (detail.isNotEmpty()) {
        Text(detail, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
    Text(
        "Named by ${song.by}",
        style = MaterialTheme.typography.bodySmall,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
    )
    Spacer(Modifier.height(16.dp))
    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        Button(
            onClick = {
                resonate.features.searchAsked.value = "${song.title} ${song.artist}".trim()
                navigator.select(Screen.Search)
                dismiss()
            },
            modifier = Modifier.testTag("find-heard"),
        ) {
            Text("Find it")
        }
        if (song.link.startsWith("http")) {
            OutlinedButton(onClick = { resonate.host.open(android.net.Uri.parse(song.link)) }) { Text("Open") }
        }
        OutlinedButton(onClick = resonate::listen) { Text("Again") }
    }
}
