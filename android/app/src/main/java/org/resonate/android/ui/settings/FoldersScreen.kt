package org.resonate.android.ui.settings

import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Add
import androidx.compose.material.icons.rounded.CreateNewFolder
import androidx.compose.material.icons.rounded.Download
import androidx.compose.material.icons.rounded.DriveFileRenameOutline
import androidx.compose.material.icons.rounded.Folder
import androidx.compose.material.icons.rounded.FolderOff
import androidx.compose.material.icons.rounded.LinkOff
import androidx.compose.material.icons.rounded.Refresh
import androidx.compose.material3.ExtendedFloatingActionButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearWavyProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.getValue
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.resonate.android.MusicDocuments
import org.resonate.android.MusicFolder
import org.resonate.android.ui.Resonate
import org.resonate.android.ui.components.EmptyState
import org.resonate.android.ui.components.Page
import org.resonate.android.ui.components.SectionHeader
import org.resonate.android.ui.components.Sizes
import org.resonate.android.ui.navigation.LocalNavigator
import org.resonate.android.ui.navigation.Screen
import org.resonate.android.ui.resonateViewModel

class FoldersViewModel(private val resonate: Resonate) : ViewModel() {
    private val features = resonate.features

    val folders = features.folders
    val scanning = resonate.library.scanning

    fun refresh() = features.refreshFolders()

    fun add() = resonate.host.chooseFolder()

    fun useDownloads() = resonate.host.useDownloads()

    fun remove(folder: MusicFolder) = features.removeFolder(folder)

    fun rescan() = resonate.library.rescan()
}

enum class FolderAccess(val text: String) {
    DOWNLOADS("Read through Android's media library · tap to scan again"),
    READ_AND_WRITE("Read and write access"),
    READ_ONLY("Read access · songs from other devices and downloads cannot land here"),
    UNAVAILABLE("Access unavailable · add the folder again to read it"),
}

fun folderAccess(downloads: Boolean, readable: Boolean, writable: Boolean): FolderAccess =
    when {
        downloads -> FolderAccess.DOWNLOADS
        !readable -> FolderAccess.UNAVAILABLE
        writable -> FolderAccess.READ_AND_WRITE
        else -> FolderAccess.READ_ONLY
    }

fun removalText(name: String, downloads: Boolean): String =
    if (downloads)
        "Resonate stops reading Downloads. Your downloaded files and their songs in your library stay as they are."
    else
        "Resonate gives back its access to “$name”. The folder and its files stay untouched on your device; its songs leave your library until you add it again."

@Composable
fun FoldersScreen() {
    val model = resonateViewModel { FoldersViewModel(it) }
    val navigator = LocalNavigator.current
    val folders by model.folders.collectAsStateWithLifecycle()
    val scanning by model.scanning.collectAsStateWithLifecycle()
    var removing by rememberSaveable { mutableStateOf<String?>(null) }
    LaunchedEffect(Unit) { model.refresh() }
    val hasDownloads = folders.any { it.uri == MusicDocuments.downloads }
    Page(
        "Music folders",
        actions = {
            IconButton(onClick = model::rescan, enabled = !scanning && folders.isNotEmpty()) {
                Icon(Icons.Rounded.Refresh, contentDescription = "Rescan folders")
            }
        },
        floatingActionButton = {
            ExtendedFloatingActionButton(
                onClick = model::add,
                icon = { Icon(Icons.Rounded.Add, contentDescription = null) },
                text = { Text("Add folder") },
                modifier = Modifier.testTag("add-folder"),
            )
        },
    ) { padding ->
        SettingsList(padding.withBottom(FAB_ROOM), Modifier.testTag("folders-list")) {
            if (scanning) {
                item(key = "scanning") {
                    LinearWavyProgressIndicator(
                        Modifier.padding(horizontal = Sizes.gutter, vertical = 8.dp)
                            .fillMaxWidth()
                    )
                }
            }
            if (folders.isEmpty()) {
                item(key = "empty") {
                    EmptyState(
                        Icons.Rounded.CreateNewFolder,
                        "No music folders yet",
                        detail =
                            "Add a folder that holds your music, or use the songs already in Downloads.",
                        modifier = Modifier.padding(top = 24.dp),
                    )
                }
            } else {
                item(key = "header") { SectionHeader("Your folders") }
                items(folders, key = { it.uri.toString() }) { folder ->
                    val downloads = folder.uri == MusicDocuments.downloads
                    val access = folderAccess(downloads, folder.readable, folder.writable)
                    SettingRow(
                        title = folder.name,
                        icon =
                            when {
                                downloads -> Icons.Rounded.Download
                                access == FolderAccess.UNAVAILABLE -> Icons.Rounded.FolderOff
                                else -> Icons.Rounded.Folder
                            },
                        supporting = access.text,
                        supportingColor =
                            if (access == FolderAccess.UNAVAILABLE) MaterialTheme.colorScheme.error
                            else null,
                        onClick = if (downloads) model::useDownloads else null,
                        trailing = {
                            Row {
                                if (folder.writable && !downloads) {
                                    IconButton(
                                        onClick = {
                                            navigator.open(Screen.Organise(folder.uri.toString()))
                                        },
                                        modifier = Modifier.testTag("organise-folder"),
                                    ) {
                                        Icon(
                                            Icons.Rounded.DriveFileRenameOutline,
                                            contentDescription = "Organise ${folder.name}",
                                        )
                                    }
                                }
                                IconButton(onClick = { removing = folder.uri.toString() }) {
                                    Icon(
                                        Icons.Rounded.LinkOff,
                                        contentDescription = "Remove access to ${folder.name}",
                                    )
                                }
                            }
                        },
                        modifier = Modifier.testTag("folder"),
                    )
                }
            }
            if (!hasDownloads) {
                item(key = "downloads-header") { SectionHeader("Downloads") }
                item(key = "downloads") {
                    SettingRow(
                        title = "Use Downloads",
                        icon = Icons.Rounded.Download,
                        supporting =
                            "Read the songs in your Downloads folder. Android asks for music access first.",
                        onClick = model::useDownloads,
                        modifier = Modifier.testTag("use-downloads"),
                    )
                }
            }
            item(key = "note") {
                Footnote(
                    "Android does not let apps choose Downloads or the whole storage as a folder, so Downloads has its own option. For any other place, choose Add folder."
                )
            }
        }
    }
    val target = folders.firstOrNull { it.uri.toString() == removing }
    if (target != null) {
        val downloads = target.uri == MusicDocuments.downloads
        ConfirmDialog(
            title = "Remove access to ${target.name}?",
            text = removalText(target.name, downloads),
            confirm = "Remove access",
            icon = Icons.Rounded.LinkOff,
            destructive = true,
            onConfirm = { model.remove(target) },
            onDismiss = { removing = null },
        )
    }
}
