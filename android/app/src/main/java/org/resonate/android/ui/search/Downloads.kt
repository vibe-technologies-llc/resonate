package org.resonate.android.ui.search

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.expandVertically
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.shrinkVertically
import androidx.compose.foundation.clickable
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Add
import androidx.compose.material.icons.rounded.Close
import androidx.compose.material.icons.rounded.Download
import androidx.compose.material.icons.rounded.DownloadDone
import androidx.compose.material.icons.rounded.Folder
import androidx.compose.material.icons.rounded.Key
import androidx.compose.material3.ElevatedCard
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearWavyProgressIndicator
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import java.util.Locale
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn
import org.resonate.android.MusicDocuments
import org.resonate.android.MusicFolder
import org.resonate.android.nativecore.SearchSong
import org.resonate.android.ui.LocalResonate
import org.resonate.android.ui.Resonate
import org.resonate.android.ui.components.Sizes
import org.resonate.android.ui.counted
import org.resonate.android.ui.navigation.LocalNavigator
import org.resonate.android.ui.navigation.Screen

data class DownloadShown(val status: String, val running: Boolean)

class Downloads(private val resonate: Resonate, private val scope: CoroutineScope) {
    private val features = resonate.features
    private val dismissed = MutableStateFlow<String?>(null)

    val shown: StateFlow<DownloadShown?> =
        combine(features.downloading, features.downloadStatus, dismissed) { running, status, hidden ->
                when {
                    status == null -> null
                    !running && status == hidden -> null
                    else -> DownloadShown(status, running)
                }
            }
            .stateIn(scope, SharingStarted.WhileSubscribed(5_000), null)

    val bytes: StateFlow<ULong> = features.downloadedBytes

    val folders: StateFlow<List<MusicFolder>> =
        features.folders
            .map(::downloadFolders)
            .stateIn(scope, SharingStarted.WhileSubscribed(5_000), emptyList())

    val ready: StateFlow<Boolean> =
        features.providerAccount
            .map { it != null }
            .stateIn(scope, SharingStarted.WhileSubscribed(5_000), features.providerAccount.value != null)

    val running: StateFlow<Boolean> = features.downloading

    val remembered: StateFlow<MusicFolder?> =
        combine(features.downloads.rememberedFolder, features.folders) { chosen, all ->
                all.firstOrNull { it.uri.toString() == chosen && it.readable && it.writable }
            }
            .stateIn(scope, SharingStarted.WhileSubscribed(5_000), null)

    val automatic: StateFlow<Boolean> = features.downloads.automatic

    fun refresh() {
        features.refreshFolders()
    }

    fun request(songs: List<SearchSong>, choose: () -> Unit) {
        val folder = remembered.value
        if (folder == null) choose() else into(songs, folder, remember = false)
    }

    fun into(songs: List<SearchSong>, folder: MusicFolder, remember: Boolean) {
        dismissed.value = null
        if (remember) features.downloads.remember(folder.uri)
        features.download(songs, folder)
    }

    fun intoDownloads(songs: List<SearchSong>, remember: Boolean) {
        dismissed.value = null
        resonate.attempt(scope) {
            val folder = features.useDownloads()
            if (remember) features.downloads.remember(folder.uri)
            features.download(songs, folder)
        }
    }

    fun forgetFolder() {
        features.downloads.remember(null)
    }

    fun automatically(enabled: Boolean) {
        features.downloads.automatically(enabled)
    }

    fun cancel() {
        features.cancelDownload()
    }

    fun dismiss() {
        dismissed.value = features.downloadStatus.value
        features.downloads.clearSettled()
    }
}

fun downloadFolders(folders: List<MusicFolder>): List<MusicFolder> =
    folders.filter { it.readable && it.writable && it.uri != MusicDocuments.downloads }

fun megabytes(bytes: ULong): String =
    String.format(Locale.ROOT, "%.1f MB", bytes.toDouble() / 1_048_576)

@Composable
fun DownloadCard(downloads: Downloads, modifier: Modifier = Modifier) {
    val shown by downloads.shown.collectAsStateWithLifecycle()
    AnimatedVisibility(
        shown != null,
        modifier = modifier,
        enter = expandVertically() + fadeIn(),
        exit = shrinkVertically() + fadeOut(),
    ) {
        val current = shown ?: return@AnimatedVisibility
        ElevatedCard(
            Modifier.fillMaxWidth()
                .padding(horizontal = Sizes.gutter, vertical = 8.dp)
                .testTag("download-card")
                .semantics { liveRegion = LiveRegionMode.Polite }
        ) {
            ListItem(
                colors = ListItemDefaults.colors(containerColor = Color.Transparent),
                leadingContent = {
                    Icon(
                        if (current.running) Icons.Rounded.Download else Icons.Rounded.DownloadDone,
                        contentDescription = null,
                        tint = MaterialTheme.colorScheme.primary,
                    )
                },
                supportingContent =
                    if (current.running) {
                        { DownloadedBytes(downloads) }
                    } else null,
                trailingContent = {
                    if (current.running) {
                        TextButton(onClick = downloads::cancel) { Text("Cancel") }
                    } else {
                        IconButton(onClick = downloads::dismiss) {
                            Icon(Icons.Rounded.Close, contentDescription = "Dismiss")
                        }
                    }
                },
            ) {
                    Text(current.status, maxLines = 2, overflow = TextOverflow.Ellipsis)
                }
            if (current.running) {
                LinearWavyProgressIndicator(
                    Modifier.fillMaxWidth().padding(start = 16.dp, end = 16.dp, bottom = 16.dp)
                )
            }
        }
    }
}

@Composable
private fun DownloadedBytes(downloads: Downloads) {
    val bytes by downloads.bytes.collectAsStateWithLifecycle()
    Text(megabytes(bytes))
}

@Composable
fun DownloadChooser(song: SearchSong, downloads: Downloads, dismiss: () -> Unit) {
    DownloadChooser(listOf(song), downloads, dismiss)
}

@Composable
fun DownloadChooser(songs: List<SearchSong>, downloads: Downloads, dismiss: () -> Unit) {
    val navigator = LocalNavigator.current
    val host = LocalResonate.current.host
    val folders by downloads.folders.collectAsStateWithLifecycle()
    val ready by downloads.ready.collectAsStateWithLifecycle()
    var remember by rememberSaveable { mutableStateOf(false) }
    LaunchedEffect(Unit) { downloads.refresh() }
    ModalBottomSheet(onDismissRequest = dismiss) {
        Column(
            Modifier.verticalScroll(rememberScrollState())
                .navigationBarsPadding()
                .testTag("download-chooser")
        ) {
            Text(
                if (songs.size == 1) "Download “${songs.single().title}”"
                else "Download ${counted(songs.size, "song")}",
                style = MaterialTheme.typography.titleLarge,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.padding(horizontal = 24.dp),
            )
            Spacer(Modifier.height(4.dp))
            Text(
                if (!ready) "Set up a download provider first."
                else "Choose the music folder they should land in.",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(horizontal = 24.dp),
            )
            Spacer(Modifier.height(8.dp))
            if (!ready) {
                ChooserRow(Icons.Rounded.Key, "Download providers", "Sign in to TIDAL, Subsonic or another source") {
                    dismiss()
                    navigator.open(Screen.Providers)
                }
            } else {
                ChooserRow(
                    Icons.Rounded.Download,
                    "Downloads",
                    "Save directly in your Downloads folder",
                ) {
                    downloads.intoDownloads(songs, remember)
                    dismiss()
                }
                folders.forEach { folder ->
                    ChooserRow(Icons.Rounded.Folder, folder.name, null) {
                        downloads.into(songs, folder, remember)
                        dismiss()
                    }
                }
                HorizontalDivider(Modifier.padding(vertical = 8.dp))
                ListItem(
                    modifier =
                        Modifier.toggleable(
                                value = remember,
                                role = Role.Switch,
                                onValueChange = { remember = it },
                            )
                            .testTag("remember-folder"),
                    colors = ListItemDefaults.colors(containerColor = Color.Transparent),
                    supportingContent = { Text("Later downloads go straight to the folder you choose.") },
                    trailingContent = { Switch(checked = remember, onCheckedChange = null) },
                ) {
                    Text("Remember this folder")
                }
                HorizontalDivider(Modifier.padding(vertical = 8.dp))
                ChooserRow(Icons.Rounded.Add, "Add music folder", "Choose another folder to download into") {
                    dismiss()
                    host.chooseFolder()
                }
            }
            Spacer(Modifier.height(16.dp))
        }
    }
}

@Composable
private fun ChooserRow(
    icon: ImageVector,
    title: String,
    detail: String?,
    enabled: Boolean = true,
    onClick: () -> Unit,
) {
    ListItem(
        modifier = Modifier.clickable(enabled = enabled, onClick = onClick),
        colors =
            ListItemDefaults.colors(
                containerColor = Color.Transparent,
                headlineColor =
                    if (enabled) MaterialTheme.colorScheme.onSurface
                    else MaterialTheme.colorScheme.onSurface.copy(alpha = 0.38f),
            ),
        leadingContent = { Icon(icon, contentDescription = null) },
        supportingContent = detail?.let { { Text(it) } },
    ) { Text(title, maxLines = 1, overflow = TextOverflow.Ellipsis) }
}
