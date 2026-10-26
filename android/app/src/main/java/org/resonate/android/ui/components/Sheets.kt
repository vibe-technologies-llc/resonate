package org.resonate.android.ui.components

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.PlaylistAdd
import androidx.compose.material.icons.automirrored.rounded.QueueMusic
import androidx.compose.material.icons.rounded.Add
import androidx.compose.material.icons.rounded.Album
import androidx.compose.material.icons.rounded.Bedtime
import androidx.compose.material.icons.rounded.DeleteOutline
import androidx.compose.material.icons.rounded.Edit
import androidx.compose.material.icons.rounded.Equalizer
import androidx.compose.material.icons.rounded.Favorite
import androidx.compose.material.icons.rounded.FavoriteBorder
import androidx.compose.material.icons.rounded.GraphicEq
import androidx.compose.material.icons.rounded.Info
import androidx.compose.material.icons.rounded.Insights
import androidx.compose.material.icons.rounded.Lyrics
import androidx.compose.material.icons.rounded.Person
import androidx.compose.material.icons.automirrored.rounded.PlaylistPlay
import androidx.compose.material.icons.rounded.RemoveCircleOutline
import androidx.compose.material.icons.rounded.Share
import androidx.compose.material.icons.rounded.Speaker
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.SnackbarDuration
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.SnackbarResult
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.testTagsAsResourceId
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.resonate.android.nativecore.SavedSearch
import org.resonate.android.nativecore.Track
import org.resonate.android.nativecore.albumKey
import org.resonate.android.ui.LocalResonate
import org.resonate.android.ui.Resonate
import org.resonate.android.ui.albumOwner
import org.resonate.android.ui.counted
import org.resonate.android.ui.navigation.LocalNavigator
import org.resonate.android.ui.navigation.Navigator
import org.resonate.android.ui.navigation.Screen
import org.resonate.android.ui.shownArtist
import org.resonate.android.ui.library.SavedSearchDialog
import org.resonate.android.ui.library.SavedSearchDraft
import org.resonate.android.ui.library.SearchEditing
import org.resonate.android.ui.shownTitle
import org.resonate.android.DeletionSentences

data class SongSheetRequest(val track: Track, val queueId: ULong? = null, val inPlayer: Boolean = false)

data class NamingRequest(val title: String, val initial: String, val save: (String) -> Unit)

@Stable
class Sheets(val snackbar: SnackbarHostState) {
    var song by mutableStateOf<SongSheetRequest?>(null)
    var picker by mutableStateOf<List<Track>?>(null)
    var naming by mutableStateOf<NamingRequest?>(null)
    var searching by mutableStateOf<SearchEditing?>(null)
    var deleting by mutableStateOf<List<Track>?>(null)
    var editingTags by mutableStateOf<List<Track>?>(null)
    var listening by mutableStateOf(false)
    var sleeping by mutableStateOf(false)

    fun listen() {
        listening = true
    }

    fun song(track: Track, queueId: ULong? = null, inPlayer: Boolean = false) {
        song = SongSheetRequest(track, queueId, inPlayer)
    }

    fun addToPlaylist(tracks: List<Track>) {
        picker = tracks
    }

    fun editTags(tracks: List<Track>) {
        editingTags = tracks
    }

    fun name(title: String, initial: String = "", save: (String) -> Unit) {
        naming = NamingRequest(title, initial, save)
    }

    fun editSearch(title: String, draft: SavedSearchDraft, save: (String, SavedSearch) -> Unit) {
        searching = SearchEditing(title, draft, save)
    }
}

val LocalSheets = staticCompositionLocalOf<Sheets> { error("No sheets in composition") }

fun Resonate.favourite(
    track: Track,
    enabled: Boolean,
    sheets: Sheets,
    scope: CoroutineScope,
) {
    library.setFavourite(track.id, enabled)
    if (!enabled) return
    scope.launch {
        val answer =
            sheets.snackbar.showSnackbar(
                "Added to favourites",
                actionLabel = "Add to playlist",
                duration = SnackbarDuration.Short,
            )
        if (answer == SnackbarResult.ActionPerformed) sheets.addToPlaylist(listOf(track))
    }
}

fun Navigator.openAlbumOf(track: Track) {
    val key = albumKey(track.album, track.artist, track.albumArtist) ?: return
    open(Screen.Album(key, track.album, track.albumOwner, track.id))
}

fun Navigator.openArtistOf(track: Track) {
    open(Screen.Artist(track.artist, track.id))
}

fun Resonate.shareLink(track: Track, scope: CoroutineScope) {
    attempt(scope) { native ->
        val link = withContext(Dispatchers.IO) { native.shareLink(track.id) }
        if (link == null) library.notice("There is no link to share for “${track.shownTitle}”")
        else host.share(track.shownTitle, link)
    }
}

@Composable
fun SheetHost() {
    val sheets = LocalSheets.current
    sheets.song?.let { request -> SongSheet(request) { sheets.song = null } }
    sheets.picker?.let { tracks -> PlaylistPicker(tracks) { sheets.picker = null } }
    sheets.naming?.let { request -> NamePlaylistDialog(request) { sheets.naming = null } }
    sheets.searching?.let { request -> SavedSearchDialog(request) { sheets.searching = null } }
    sheets.deleting?.let { tracks -> DeleteSongsDialog(tracks) { sheets.deleting = null } }
    sheets.editingTags?.let { tracks -> TagEditorSheet(tracks) { sheets.editingTags = null } }
    if (sheets.listening) ListenSheet { sheets.listening = false }
}

@Composable
private fun DeleteSongsDialog(tracks: List<Track>, dismiss: () -> Unit) {
    val resonate = LocalResonate.current
    AlertDialog(
        onDismissRequest = dismiss,
        icon = { Icon(Icons.Rounded.DeleteOutline, contentDescription = null) },
        title = { Text(DeletionSentences.title(tracks.map { it.shownTitle })) },
        text = {
            Text(
                if (tracks.size == 1)
                    "Its file is deleted from your phone, and it leaves the queue, your playlists and favourites."
                else
                    "Their files are deleted from your phone, and they leave the queue, your playlists and favourites."
            )
        },
        confirmButton = {
            TextButton(
                onClick = {
                    resonate.deleteSongs(tracks)
                    dismiss()
                },
                colors = ButtonDefaults.textButtonColors(contentColor = MaterialTheme.colorScheme.error),
                modifier = Modifier.testTag("confirm-delete"),
            ) {
                Text(if (tracks.size == 1) "Delete song" else "Delete ${tracks.size} songs")
            }
        },
        dismissButton = { TextButton(onClick = dismiss) { Text("Cancel") } },
    )
}

@Composable
private fun SongSheet(request: SongSheetRequest, dismiss: () -> Unit) {
    val resonate = LocalResonate.current
    val navigator = LocalNavigator.current
    val sheets = LocalSheets.current
    val scope = rememberCoroutineScope()
    val catalog by resonate.library.allTracks.collectAsState()
    val track = remember(catalog, request) { resonate.library.findTrack(request.track.id) ?: request.track }
    val actions = buildList {
        add(
            SheetAction(
                if (track.favourite) Icons.Rounded.Favorite else Icons.Rounded.FavoriteBorder,
                if (track.favourite) "Remove from favourites" else "Add to favourites",
            ) {
                resonate.favourite(track, !track.favourite, sheets, scope)
            }
        )
        add(SheetAction(Icons.AutoMirrored.Rounded.PlaylistPlay, "Play next") {
            resonate.library.request { it.playNext(listOf(track.id)) }
        })
        add(SheetAction(Icons.AutoMirrored.Rounded.QueueMusic, "Add to queue") {
            resonate.library.request { it.addToQueue(listOf(track.id)) }
        })
        add(SheetAction(Icons.AutoMirrored.Rounded.PlaylistAdd, "Add to playlist") {
            sheets.addToPlaylist(listOf(track))
        })
        if (track.album.isNotBlank()) {
            add(SheetAction(Icons.Rounded.Album, "Go to album") { navigator.openAlbumOf(track) })
        }
        add(SheetAction(Icons.Rounded.Person, "Go to artist") { navigator.openArtistOf(track) })
        add(SheetAction(Icons.Rounded.Lyrics, "Lyrics") { navigator.open(Screen.Lyrics(track.id)) })
        add(SheetAction(Icons.Rounded.Insights, "Analysis") {
            navigator.open(Screen.Analysis(track.id))
        })
        add(SheetAction(Icons.Rounded.Info, "Song details") {
            navigator.open(Screen.Details(track.id))
        })
        add(SheetAction(Icons.Rounded.Share, "Share link") { resonate.shareLink(track, scope) })
        if (resonate.features.tags.writable(track)) {
            add(SheetAction(Icons.Rounded.Edit, "Edit tags") { sheets.editTags(listOf(track)) })
        }
        if (resonate.features.songDeletable(track)) {
            add(SheetAction(Icons.Rounded.DeleteOutline, "Delete song", destructive = true) {
                sheets.deleting = listOf(track)
            })
        }
        if (request.inPlayer) {
            add(SheetAction(Icons.Rounded.Bedtime, "Sleep timer") { sheets.sleeping = true })
            add(SheetAction(Icons.Rounded.Equalizer, "Equaliser") { navigator.open(Screen.Equaliser) })
            add(SheetAction(Icons.Rounded.Speaker, "Audio output") { navigator.open(Screen.Output) })
            add(SheetAction(Icons.Rounded.GraphicEq, "Visualizer") {
                navigator.open(Screen.Visualizer)
            })
        }
        request.queueId?.let { row ->
            add(SheetAction(Icons.Rounded.RemoveCircleOutline, "Remove from queue") {
                resonate.library.request { it.removeFromQueue(row) }
            })
        }
    }
    ModalBottomSheet(onDismissRequest = dismiss) {
        Column(
            Modifier.verticalScroll(rememberScrollState()).navigationBarsPadding().semantics {
                testTagsAsResourceId = true
            }
        ) {
            ListItem(
                colors = ListItemDefaults.colors(containerColor = Color.Transparent),
                leadingContent = { Artwork(track, Sizes.rowArt, shape = MaterialTheme.shapes.medium) },
                supportingContent = { Text(track.shownArtist, maxLines = 1) },
            ) {
                Text(
                    track.shownTitle,
                    style = MaterialTheme.typography.titleMedium,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            HorizontalDivider(Modifier.padding(vertical = 8.dp))
            actions.forEach { action ->
                SheetRow(action.icon, action.label, destructive = action.destructive) {
                    dismiss()
                    action.run()
                }
            }
        }
    }
}

private class SheetAction(
    val icon: ImageVector,
    val label: String,
    val destructive: Boolean = false,
    val run: () -> Unit,
)

@Composable
fun SheetRow(icon: ImageVector, label: String, destructive: Boolean = false, onClick: () -> Unit) {
    val ink = if (destructive) MaterialTheme.colorScheme.error else Color.Unspecified
    ListItem(
        onClick = onClick,
        colors =
            ListItemDefaults.colors(
                containerColor = Color.Transparent,
                contentColor = if (destructive) ink else MaterialTheme.colorScheme.onSurface,
                leadingContentColor = if (destructive) ink else MaterialTheme.colorScheme.onSurfaceVariant,
            ),
        leadingContent = { Icon(icon, contentDescription = null) },
    ) {
        Text(label)
    }
}

@Composable
private fun PlaylistPicker(tracks: List<Track>, dismiss: () -> Unit) {
    val resonate = LocalResonate.current
    val sheets = LocalSheets.current
    val playlists by resonate.features.playlists.collectAsState()
    LaunchedEffect(Unit) { resonate.features.loadPlaylists() }
    ModalBottomSheet(onDismissRequest = dismiss) {
        Text(
            "Add to playlist",
            style = MaterialTheme.typography.titleLarge,
            modifier = Modifier.padding(horizontal = 24.dp, vertical = 8.dp),
        )
        LazyColumn(
            Modifier.fillMaxWidth().navigationBarsPadding().semantics { testTagsAsResourceId = true }
        ) {
            item {
                CollectionRow(
                    title = "New playlist",
                    supporting = null,
                    onClick = {
                        sheets.name("New playlist") { resonate.features.createPlaylist(it) }
                    },
                    leading = { IconTile(Icons.Rounded.Add) },
                )
            }
            items(playlists.filter { it.search == null }, key = { it.id.toString() }) { playlist ->
                CollectionRow(
                    title = playlist.name,
                    supporting = counted(playlist.songs, "song"),
                    onClick = {
                        resonate.features.addToPlaylist(playlist.id, tracks)
                        dismiss()
                    },
                    leading = {
                        PlaylistCover(playlist.covers, Sizes.rowArt, shape = MaterialTheme.shapes.medium)
                    },
                )
            }
        }
    }
}

@Composable
private fun NamePlaylistDialog(request: NamingRequest, dismiss: () -> Unit) {
    var name by rememberSaveable { mutableStateOf(request.initial) }
    val focus = remember { FocusRequester() }
    val save = {
        val chosen = name.trim()
        if (chosen.isNotEmpty()) {
            request.save(chosen)
            dismiss()
        }
    }
    LaunchedEffect(Unit) { focus.requestFocus() }
    AlertDialog(
        onDismissRequest = dismiss,
        title = { Text(request.title) },
        text = {
            OutlinedTextField(
                value = name,
                onValueChange = { name = it.take(PLAYLIST_NAME_AT_MOST) },
                label = { Text("Playlist name") },
                singleLine = true,
                keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done),
                keyboardActions = KeyboardActions(onDone = { save() }),
                modifier = Modifier.focusRequester(focus),
            )
        },
        confirmButton = {
            TextButton(onClick = save, enabled = name.isNotBlank()) { Text("Save") }
        },
        dismissButton = { TextButton(onClick = dismiss) { Text("Cancel") } },
    )
}

private const val PLAYLIST_NAME_AT_MOST = 200
