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
import androidx.compose.material.icons.automirrored.rounded.PlaylistPlay
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
import androidx.compose.material.icons.rounded.QueryStats
import androidx.compose.material.icons.rounded.Lyrics
import androidx.compose.material.icons.rounded.Person
import androidx.compose.material.icons.rounded.RemoveCircleOutline
import androidx.compose.material.icons.rounded.Share
import androidx.compose.material.icons.rounded.Speaker
import androidx.compose.material.icons.rounded.SwapHoriz
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.ColorScheme
import androidx.compose.material3.HorizontalDivider
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
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.testTagsAsResourceId
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.resonate.android.DeletionSentences
import org.resonate.android.R
import org.resonate.android.nativecore.SavedSearch
import org.resonate.android.nativecore.Track
import org.resonate.android.nativecore.albumKey
import org.resonate.android.served
import org.resonate.android.ui.LocalResonate
import org.resonate.android.ui.Resonate
import org.resonate.android.ui.albumOwner
import org.resonate.android.ui.library.SavedSearchDialog
import org.resonate.android.ui.library.SavedSearchDraft
import org.resonate.android.ui.library.SearchEditing
import org.resonate.android.ui.navigation.LocalNavigator
import org.resonate.android.ui.navigation.Navigator
import org.resonate.android.ui.navigation.Screen
import org.resonate.android.ui.shownArtist
import org.resonate.android.ui.shownTitle
import org.resonate.android.ui.theme.ArtworkTheme
import org.resonate.android.words

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
    var versions by mutableStateOf<Track?>(null)
    var listening by mutableStateOf(false)
    var sleeping by mutableStateOf(false)
    var worn by mutableStateOf<ColorScheme?>(null)

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
    if (track.served) {
        attempt(scope) { native ->
            features.served.bind(native)
            withContext(Dispatchers.IO) { native.favourServedSong(track.uri, enabled) }
        }
        return
    }
    library.setFavourite(track.id, enabled)
    if (!enabled) return
    scope.launch {
        val answer =
            sheets.snackbar.showSnackbar(
                library.words.string(R.string.sheet_added_to_favourites),
                actionLabel = library.words.string(R.string.sheet_add_to_playlist),
                duration = SnackbarDuration.Long,
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
        val link = try {
            withContext(Dispatchers.IO) { native.shareLink(track.id) }
        } catch (cancelled: kotlinx.coroutines.CancellationException) {
            throw cancelled
        } catch (failure: Exception) {
            val message = failure.message
            if (message != null) library.notice(message)
            else library.notice(R.string.sheet_share_unresolved)
            return@attempt
        }
        val title = track.shownTitle(library.words)
        if (link == null) library.notice(R.string.sheet_share_none, title)
        else host.share(title, link)
    }
}

@Composable
fun SheetHost() {
    val sheets = LocalSheets.current
    ArtworkTheme(sheets.worn ?: MaterialTheme.colorScheme) {
        sheets.song?.let { request -> SongSheet(request) { sheets.song = null } }
        sheets.picker?.let { tracks -> PlaylistPicker(tracks) { sheets.picker = null } }
        sheets.naming?.let { request -> NamePlaylistDialog(request) { sheets.naming = null } }
        sheets.searching?.let { request -> SavedSearchDialog(request) { sheets.searching = null } }
        sheets.deleting?.let { tracks -> DeleteSongsDialog(tracks) { sheets.deleting = null } }
        sheets.editingTags?.let { tracks -> TagEditorSheet(tracks) { sheets.editingTags = null } }
        sheets.versions?.let { track -> VersionsSheet(track) { sheets.versions = null } }
        if (sheets.listening) ListenSheet { sheets.listening = false }
    }
}

@Composable
private fun DeleteSongsDialog(tracks: List<Track>, dismiss: () -> Unit) {
    val resonate = LocalResonate.current
    AlertDialog(
        onDismissRequest = dismiss,
        icon = { Icon(Icons.Rounded.DeleteOutline, contentDescription = null) },
        title = {
            Text(DeletionSentences.title(words(), tracks.map { it.shownTitle }))
        },
        text = {
            Text(pluralStringResource(R.plurals.sheet_delete_explained, tracks.size))
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
                Text(
                    if (tracks.size == 1) stringResource(R.string.sheet_delete_song)
                    else
                        pluralStringResource(R.plurals.sheet_delete_songs, tracks.size, tracks.size)
                )
            }
        },
        dismissButton = {
            TextButton(onClick = dismiss) { Text(stringResource(R.string.sheet_cancel)) }
        },
    )
}

@Composable
private fun SongSheet(request: SongSheetRequest, dismiss: () -> Unit) {
    val resonate = LocalResonate.current
    val navigator = LocalNavigator.current
    val sheets = LocalSheets.current
    val scope = rememberCoroutineScope()
    val words = words()
    val catalog by resonate.library.allTracks.collectAsState()
    val track =
        remember(catalog, request) {
            if (request.track.served) request.track
            else resonate.library.findTrack(request.track.id) ?: request.track
        }
    val actions = buildList {
        if (!track.served) {
            add(
                SheetAction(
                    if (track.favourite) Icons.Rounded.Favorite else Icons.Rounded.FavoriteBorder,
                    words.string(
                        if (track.favourite) R.string.player_favourite_remove
                        else R.string.player_favourite_add
                    ),
                ) {
                    resonate.favourite(track, !track.favourite, sheets, scope)
                }
            )
            add(SheetAction(
                Icons.AutoMirrored.Rounded.PlaylistPlay,
                words.string(R.string.sheet_play_next),
            ) {
                resonate.library.request { it.playNext(listOf(track.id)) }
            })
            add(SheetAction(
                Icons.AutoMirrored.Rounded.QueueMusic,
                words.string(R.string.sheet_add_to_queue),
            ) {
                resonate.library.request { it.addToQueue(listOf(track.id)) }
            })
            add(SheetAction(
                Icons.AutoMirrored.Rounded.PlaylistAdd,
                words.string(R.string.sheet_add_to_playlist),
            ) {
                sheets.addToPlaylist(listOf(track))
            })
            if (track.album.isNotBlank()) {
                add(SheetAction(Icons.Rounded.Album, words.string(R.string.player_go_to_album)) {
                    navigator.openAlbumOf(track)
                })
            }
            add(SheetAction(Icons.Rounded.Person, words.string(R.string.player_go_to_artist)) {
                navigator.openArtistOf(track)
            })
            add(SheetAction(Icons.Rounded.Lyrics, words.string(R.string.player_lyrics)) {
                navigator.open(Screen.Lyrics(track.id))
            })
            add(SheetAction(Icons.Rounded.QueryStats, words.string(R.string.analysis_title)) {
                navigator.open(Screen.Analysis(track.id))
            })
            add(SheetAction(Icons.Rounded.Info, words.string(R.string.details_title)) {
                navigator.open(Screen.Details(track.id))
            })
            add(SheetAction(Icons.Rounded.Share, words.string(R.string.sheet_share_link)) {
                resonate.shareLink(track, scope)
            })
            add(SheetAction(
                Icons.Rounded.SwapHoriz,
                words.string(R.string.sheet_find_another_version),
            ) {
                sheets.versions = track
            })
            if (resonate.features.tags.writable(track)) {
                add(SheetAction(Icons.Rounded.Edit, words.string(R.string.sheet_edit_tags)) {
                    sheets.editTags(listOf(track))
                })
            }
            if (resonate.features.songDeletable(track)) {
                add(SheetAction(
                    Icons.Rounded.DeleteOutline,
                    words.string(R.string.sheet_delete_song),
                    destructive = true,
                ) {
                    sheets.deleting = listOf(track)
                })
            }
        }
        if (request.inPlayer) {
            add(SheetAction(Icons.Rounded.Bedtime, words.string(R.string.player_sleep_timer)) {
                sheets.sleeping = true
            })
            add(SheetAction(Icons.Rounded.Equalizer, words.string(R.string.sheet_equaliser)) {
                navigator.open(Screen.Equaliser)
            })
            add(SheetAction(Icons.Rounded.Speaker, words.string(R.string.sheet_audio_output)) {
                navigator.open(Screen.Output)
            })
            add(SheetAction(Icons.Rounded.GraphicEq, words.string(R.string.sheet_visualizer)) {
                navigator.open(Screen.Visualizer)
            })
        }
        request.queueId?.let { row ->
            add(SheetAction(
                Icons.Rounded.RemoveCircleOutline,
                words.string(R.string.queue_remove),
            ) {

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
    val newPlaylist = stringResource(R.string.sheet_new_playlist)
    ModalBottomSheet(onDismissRequest = dismiss) {
        Text(
            stringResource(R.string.sheet_add_to_playlist),
            style = MaterialTheme.typography.titleLarge,
            modifier = Modifier.padding(horizontal = 24.dp, vertical = 8.dp),
        )
        LazyColumn(
            Modifier.fillMaxWidth().navigationBarsPadding().semantics { testTagsAsResourceId = true }
        ) {
            item {
                CollectionRow(
                    title = newPlaylist,
                    supporting = null,
                    onClick = {
                        sheets.name(newPlaylist) { name ->
                            resonate.features.createPlaylist(name, tracks)
                            dismiss()
                        }
                    },
                    leading = { IconTile(Icons.Rounded.Add) },
                )
            }
            items(playlists.filter { it.search == null }, key = { it.id.toString() }) { playlist ->
                CollectionRow(
                    title = playlist.name,
                    supporting =
                        pluralStringResource(
                            R.plurals.count_songs,
                            playlist.songs.toInt(),
                            playlist.songs.toInt(),
                        ),
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
                label = { Text(stringResource(R.string.sheet_playlist_name)) },
                singleLine = true,
                keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done),
                keyboardActions = KeyboardActions(onDone = { save() }),
                modifier = Modifier.focusRequester(focus),
            )
        },
        confirmButton = {
            TextButton(onClick = save, enabled = name.isNotBlank()) {
                Text(stringResource(R.string.sheet_save))
            }
        },
        dismissButton = {
            TextButton(onClick = dismiss) { Text(stringResource(R.string.sheet_cancel)) }
        },

    )
}

private const val PLAYLIST_NAME_AT_MOST = 200
