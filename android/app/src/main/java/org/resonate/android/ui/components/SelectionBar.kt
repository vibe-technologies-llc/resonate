package org.resonate.android.ui.components

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.PlaylistAdd
import androidx.compose.material.icons.automirrored.rounded.PlaylistPlay
import androidx.compose.material.icons.automirrored.rounded.QueueMusic
import androidx.compose.material.icons.rounded.Close
import androidx.compose.material.icons.rounded.DeleteOutline
import androidx.compose.material.icons.rounded.Favorite
import androidx.compose.material.icons.rounded.FavoriteBorder
import androidx.compose.material.icons.rounded.PlayArrow
import androidx.compose.material.icons.rounded.RemoveCircleOutline
import androidx.compose.material.icons.rounded.SelectAll
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import org.resonate.android.nativecore.Track
import org.resonate.android.ui.LocalResonate
import org.resonate.android.ui.counted
import org.resonate.android.ui.library.MenuEntry
import org.resonate.android.ui.library.OverflowMenu
import org.resonate.android.ui.library.addToQueue
import org.resonate.android.ui.library.playInOrder
import org.resonate.android.ui.library.playNext

class SelectionRemoval(val label: String, val run: (List<Track>) -> Unit)

@Composable
fun SongSelectionBar(
    tracks: List<Track>,
    onSelectAll: () -> Unit,
    onClose: () -> Unit,
    modifier: Modifier = Modifier,
    removal: SelectionRemoval? = null,
    windowInsets: WindowInsets = TopAppBarDefaults.windowInsets,
) {
    val resonate = LocalResonate.current
    val sheets = LocalSheets.current
    val catalog by resonate.library.allTracks.collectAsState()
    val live = remember(catalog, tracks) { tracks.map { resonate.library.findTrack(it.id) ?: it } }
    val loved = live.isNotEmpty() && live.all { it.favourite }
    val deletable = live.isNotEmpty() && live.all(resonate.features::songDeletable)
    BackHandler(onBack = onClose)
    TopAppBar(
        title = {
            Text(
                "${tracks.size} selected",
                modifier = Modifier.semantics { liveRegion = LiveRegionMode.Polite },
            )
        },
        modifier = modifier.testTag("selection-bar"),
        windowInsets = windowInsets,
        navigationIcon = {
            IconButton(onClick = onClose, modifier = Modifier.testTag("selection-close")) {
                Icon(Icons.Rounded.Close, contentDescription = "Clear selection")
            }
        },
        actions = {
            IconButton(onClick = onSelectAll, modifier = Modifier.testTag("selection-all")) {
                Icon(Icons.Rounded.SelectAll, contentDescription = "Select all")
            }
            IconButton(
                onClick = {
                    resonate.playInOrder(live)
                    onClose()
                },
                modifier = Modifier.testTag("selection-play"),
            ) {
                Icon(Icons.Rounded.PlayArrow, contentDescription = "Play")
            }
            IconButton(
                onClick = {
                    sheets.addToPlaylist(live)
                    onClose()
                },
                modifier = Modifier.testTag("selection-playlist"),
            ) {
                Icon(Icons.AutoMirrored.Rounded.PlaylistAdd, contentDescription = "Add to playlist")
            }
            IconButton(
                onClick = {
                    resonate.library.setFavourites(live.map { it.id }, !loved)
                    onClose()
                },
                modifier = Modifier.testTag("selection-favourite"),
            ) {
                Icon(
                    if (loved) Icons.Rounded.Favorite else Icons.Rounded.FavoriteBorder,
                    contentDescription = if (loved) "Remove from favourites" else "Add to favourites",
                )
            }
            OverflowMenu(
                listOf(
                    listOf(
                        MenuEntry.Action("Play next", Icons.AutoMirrored.Rounded.PlaylistPlay) {
                            resonate.playNext(live)
                            onClose()
                        },
                        MenuEntry.Action("Add to queue", Icons.AutoMirrored.Rounded.QueueMusic) {
                            resonate.addToQueue(live)
                            onClose()
                        },
                    ),
                    listOfNotNull(
                        removal?.let { held ->
                            MenuEntry.Action(held.label, Icons.Rounded.RemoveCircleOutline) {
                                held.run(tracks)
                                onClose()
                            }
                        },
                        if (deletable)
                            MenuEntry.Action(
                                if (live.size == 1) "Delete song" else "Delete ${counted(live.size, "song")}",
                                Icons.Rounded.DeleteOutline,
                                destructive = true,
                            ) {
                                sheets.deleting = live
                                onClose()
                            }
                        else null,
                    ),
                ),
                description = "More actions",
            )
        },
        colors =
            TopAppBarDefaults.topAppBarColors(
                containerColor = MaterialTheme.colorScheme.secondaryContainer,
                scrolledContainerColor = MaterialTheme.colorScheme.secondaryContainer,
            ),
    )
}
