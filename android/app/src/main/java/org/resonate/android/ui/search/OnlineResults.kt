package org.resonate.android.ui.search

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.ArrowBack
import androidx.compose.material.icons.automirrored.rounded.PlaylistAdd
import androidx.compose.material.icons.outlined.BookmarkAdd
import androidx.compose.material.icons.outlined.BookmarkBorder
import androidx.compose.material.icons.rounded.Album
import androidx.compose.material.icons.rounded.CloudOff
import androidx.compose.material.icons.rounded.LinkOff
import androidx.compose.material.icons.rounded.MusicOff
import androidx.compose.material.icons.rounded.Person
import androidx.compose.material.icons.rounded.SearchOff
import androidx.compose.material.icons.rounded.TravelExplore
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.resonate.android.nativecore.SearchAlbum
import org.resonate.android.nativecore.SearchArtist
import org.resonate.android.nativecore.SearchRelease
import org.resonate.android.nativecore.SearchSong
import org.resonate.android.ui.components.EmptyState
import org.resonate.android.ui.components.IconTile
import org.resonate.android.ui.components.LocalSheets
import org.resonate.android.ui.components.SectionHeader
import org.resonate.android.ui.components.Sizes
import org.resonate.android.ui.counted
import org.resonate.android.ui.navigation.LocalNavigator
import org.resonate.android.ui.navigation.Screen

@Stable
class OnlineScroll {
    var current by mutableStateOf<LazyListState?>(null)

    val scrolled: Boolean
        get() = current?.canScrollBackward == true

    val moving: Boolean
        get() = current?.isScrollInProgress == true
}

@Composable
fun OnlineResults(
    model: SearchViewModel,
    scroll: OnlineScroll,
    padding: PaddingValues,
    act: (SearchSong) -> Unit,
) {
    val trail by model.trail.collectAsStateWithLifecycle()
    val shown by model.onlineShown.collectAsStateWithLifecycle()
    val typed by model.typed.collectAsStateWithLifecycle()
    val recent by model.recent.collectAsStateWithLifecycle()
    val navigator = LocalNavigator.current
    val sheets = LocalSheets.current
    Box(Modifier.fillMaxSize(), contentAlignment = Alignment.TopCenter) {
        key(trail.views.size, trail.top) {
            val state = rememberLazyListState()
            SideEffect { scroll.current = state }
            LazyColumn(
                state = state,
                contentPadding = padding,
                modifier = Modifier.widthIn(max = ContentWidth).fillMaxSize().testTag("online-results"),
            ) {
                item(key = "download") { DownloadCard(model.downloads) }
                val top = trail.top
                if (trail.nested) item(key = "heading") { Heading(top, model::back) }
                when {
                    typed.isEmpty() && top is OnlineView.Found ->
                        explore(
                            recent = recent,
                            search = model::search,
                            forget = model::forget,
                            forgetAll = model::forgetAll,
                            wanted = { navigator.open(Screen.Wanted) },
                        )
                    else -> {
                        if (top is OnlineView.Found) {
                            item(key = "kinds") { KindChips(trail.kind, model::choose) }
                        }
                        when (val current = shown) {
                            OnlineShown.Idle ->
                                item(key = "short") {
                                    EmptyState(
                                        Icons.Rounded.TravelExplore,
                                        "Keep typing",
                                        Modifier.fillParentMaxHeight(0.5f),
                                        detail = "Type at least two letters to search online.",
                                    )
                                }
                            OnlineShown.Loading -> loading()
                            is OnlineShown.Failed ->
                                item(key = "failed") {
                                    EmptyState(
                                        Icons.Rounded.CloudOff,
                                        "Couldn’t reach the music service",
                                        Modifier.fillParentMaxHeight(0.6f),
                                        detail = current.reason ?: "Check your connection and try again.",
                                        action = "Try again",
                                        onAction = model::retryOnline,
                                    )
                                }
                            is OnlineShown.Ready ->
                                answer(
                                    top = top,
                                    answer = current.answer,
                                    loadingMore = current.more,
                                    model = model,
                                    act = act,
                                    import = { name, songs ->
                                        sheets.name("Import as playlist", name) { model.import(it, songs) }
                                    },
                                )
                        }
                    }
                }
            }
        }
    }
}

private fun LazyListScope.explore(
    recent: List<String>,
    search: (String) -> Unit,
    forget: (String) -> Unit,
    forgetAll: () -> Unit,
    wanted: () -> Unit,
) {
    item(key = "wanted") {
        ListItem(
            modifier = Modifier.padding(top = 4.dp).clickable(onClick = wanted).testTag("wanted-row"),
            colors = ListItemDefaults.colors(containerColor = Color.Transparent),
            leadingContent = { IconTile(Icons.Outlined.BookmarkBorder) },
            supportingContent = { Text("Songs you saved from online search") },
        ) { Text("Saved for later") }
    }
    recentSearches(recent, search, forget, forgetAll)
    item(key = "explore") {
        EmptyState(
            Icons.Rounded.TravelExplore,
            "Find music online",
            Modifier.padding(top = 24.dp),
            detail =
                "Search MusicBrainz for songs, albums and artists to save or download, " +
                    "or paste a TIDAL, Spotify or MusicBrainz link to open it.",
        )
    }
}

@Composable
private fun KindChips(kind: OnlineKind, choose: (OnlineKind) -> Unit) {
    LazyRow(
        contentPadding = PaddingValues(horizontal = Sizes.gutter),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        modifier = Modifier.padding(vertical = 4.dp).testTag("online-kinds"),
    ) {
        items(OnlineKind.entries, key = { it.name }) { option ->
            FilterChip(
                selected = option == kind,
                onClick = { choose(option) },
                label = { Text(option.label) },
                modifier = Modifier.testTag("kind-${option.name.lowercase()}"),
            )
        }
    }
}

@Composable
private fun Heading(view: OnlineView, back: () -> Boolean) {
    val (title, detail, backLabel) =
        when (view) {
            is OnlineView.Found -> Triple(view.kind.label, "", "Back")
            is OnlineView.Discography -> Triple(view.name, "Albums and singles", "Back")
            is OnlineView.Editions -> Triple(view.title, editionsDetail(view), "Back")
            is OnlineView.Release -> Triple(view.title, view.detail, "Back to editions")
            is OnlineView.Link -> Triple("Shared link", view.text, "Close link")
        }
    Row(
        Modifier.fillMaxWidth().padding(start = 4.dp, end = Sizes.gutter, top = 4.dp, bottom = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        IconButton(onClick = { back() }, modifier = Modifier.testTag("online-back")) {
            Icon(Icons.AutoMirrored.Rounded.ArrowBack, contentDescription = backLabel)
        }
        Column(Modifier.weight(1f).padding(start = 4.dp)) {
            Text(
                title.ifBlank { "Untitled" },
                style = MaterialTheme.typography.titleLarge,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.semantics { heading() },
            )
            if (detail.isNotBlank()) {
                Text(
                    detail,
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
    }
}

private fun editionsDetail(view: OnlineView.Editions): String =
    listOf(view.artist, "Editions").filter { it.isNotBlank() }.joinToString(" · ")

private fun releaseDetail(release: SearchRelease): String =
    listOfNotNull(
            release.date.takeIf { it.isNotBlank() },
            release.country.takeIf { it.isNotBlank() },
            release.songs.takeIf { it > 0u }?.let { counted(it, "song") },
        )
        .joinToString(" · ")

private fun albumDetail(album: SearchAlbum): String =
    listOf(album.artist, album.date.take(4)).filter { it.isNotBlank() }.joinToString(" · ")

private fun LazyListScope.answer(
    top: OnlineView,
    answer: OnlineAnswer,
    loadingMore: Boolean,
    model: SearchViewModel,
    act: (SearchSong) -> Unit,
    import: (String, List<SearchSong>) -> Unit,
) {
    when (answer) {
        is OnlineAnswer.Songs -> {
            if (answer.songs.isEmpty()) {
                nothing(
                    if (top is OnlineView.Release) Icons.Rounded.MusicOff else Icons.Rounded.SearchOff,
                    if (top is OnlineView.Release) "No songs listed" else "No songs found",
                    if (top is OnlineView.Release) "This edition lists no songs."
                    else "Try a different title or artist.",
                )
                return
            }
            if (top is OnlineView.Release) {
                item(key = "release-actions") {
                    CollectionActions(
                        songs = answer.songs,
                        importable = null,
                        save = model::saveAll,
                        import = import,
                    )
                }
            }
            songs(answer.songs, numbered = top is OnlineView.Release, act = act)
        }
        is OnlineAnswer.Albums -> {
            if (answer.albums.isEmpty()) {
                nothing(Icons.Rounded.Album, "No albums found", "Try a different title or artist.")
                return
            }
            albums(answer.albums, model)
            if (top is OnlineView.Discography && answer.next != null) {
                item(key = "more") {
                    Box(Modifier.fillMaxWidth().padding(16.dp), contentAlignment = Alignment.Center) {
                        if (loadingMore) {
                            LoadingIndicator(Modifier.size(40.dp))
                        } else {
                            OutlinedButton(onClick = model::more, modifier = Modifier.testTag("more-albums")) {
                                Text("Show more albums")
                            }
                        }
                    }
                }
            }
        }
        is OnlineAnswer.Artists -> {
            if (answer.artists.isEmpty()) {
                nothing(Icons.Rounded.Person, "No artists found", "Try a different spelling.")
                return
            }
            artists(answer.artists, model)
        }
        is OnlineAnswer.Editions -> {
            if (answer.releases.isEmpty()) {
                nothing(Icons.Rounded.Album, "No editions found", "This album has no releases to show.")
                return
            }
            releases(answer.releases, model)
        }
        is OnlineAnswer.Link -> {
            if (answer.songs.isEmpty() && answer.releases.isEmpty() && answer.artists.isEmpty()) {
                nothing(Icons.Rounded.LinkOff, "Nothing to show", "No music could be read from this link.")
                return
            }
            item(key = "link-title") {
                Column(Modifier.fillMaxWidth().padding(horizontal = Sizes.gutter, vertical = 8.dp)) {
                    Text(
                        answer.name.ifBlank { "Shared music" },
                        style = MaterialTheme.typography.headlineSmall,
                        maxLines = 3,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.semantics { heading() },
                    )
                    if (answer.songs.isNotEmpty()) {
                        Text(
                            counted(answer.songs.size, "song"),
                            style = MaterialTheme.typography.bodyMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
            }
            if (answer.songs.isNotEmpty()) {
                item(key = "link-actions") {
                    CollectionActions(
                        songs = answer.songs,
                        importable = answer.name.ifBlank { "Imported playlist" },
                        save = model::saveAll,
                        import = import,
                    )
                }
                item(key = "link-songs") { SectionHeader("Songs") }
                songs(answer.songs, numbered = false, act = act)
            }
            if (answer.releases.isNotEmpty()) {
                item(key = "link-releases") { SectionHeader("Editions") }
                releases(answer.releases, model)
            }
            if (answer.artists.isNotEmpty()) {
                item(key = "link-artists") { SectionHeader("Artists") }
                artists(answer.artists, model)
            }
        }
    }
}

private fun LazyListScope.nothing(icon: ImageVector, title: String, detail: String) {
    item(key = "nothing") {
        EmptyState(icon, title, Modifier.fillParentMaxHeight(0.55f), detail = detail)
    }
}

private fun LazyListScope.songs(songs: List<SearchSong>, numbered: Boolean, act: (SearchSong) -> Unit) {
    itemsIndexed(songs, key = { index, song -> "song-$index-${song.recording}" }) { index, song ->
        OnlineSongRow(
            song = song,
            held = song.held,
            onClick = { act(song) },
            number = if (numbered) (song.position?.toString() ?: "${index + 1}") else null,
        )
    }
}

private fun LazyListScope.albums(albums: List<SearchAlbum>, model: SearchViewModel) {
    items(albums, key = { "album-${it.id}" }) { album ->
        OnlineCollectionRow(
            title = album.title,
            detail = albumDetail(album),
            icon = Icons.Rounded.Album,
            onClick = { model.open(OnlineView.Editions(album.id, album.title, album.artist)) },
            modifier = Modifier.testTag("online-album"),
        )
    }
}

private fun LazyListScope.artists(artists: List<SearchArtist>, model: SearchViewModel) {
    items(artists, key = { "artist-${it.id}" }) { artist ->
        OnlineCollectionRow(
            title = artist.name,
            detail = artist.kind,
            icon = Icons.Rounded.Person,
            round = true,
            onClick = { model.open(OnlineView.Discography(artist.id, artist.name)) },
            modifier = Modifier.testTag("online-artist"),
        )
    }
}

private fun LazyListScope.releases(releases: List<SearchRelease>, model: SearchViewModel) {
    items(releases, key = { "release-${it.id}" }) { release ->
        OnlineCollectionRow(
            title = release.title,
            detail = releaseDetail(release),
            icon = Icons.Rounded.Album,
            onClick = { model.open(OnlineView.Release(release.id, release.title, releaseDetail(release))) },
            modifier = Modifier.testTag("online-release"),
        )
    }
}

@Composable
private fun CollectionActions(
    songs: List<SearchSong>,
    importable: String?,
    save: (List<SearchSong>) -> Unit,
    import: (String, List<SearchSong>) -> Unit,
) {
    FlowRow(
        Modifier.fillMaxWidth().padding(horizontal = Sizes.gutter, vertical = 8.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        if (importable != null) {
            Button(onClick = { import(importable, songs) }, modifier = Modifier.testTag("import-playlist")) {
                Icon(Icons.AutoMirrored.Rounded.PlaylistAdd, null, Modifier.size(ButtonDefaults.IconSize))
                Spacer(Modifier.size(ButtonDefaults.IconSpacing))
                Text("Import as playlist")
            }
        }
        FilledTonalButton(onClick = { save(songs) }, modifier = Modifier.testTag("save-all")) {
            Icon(Icons.Outlined.BookmarkAdd, null, Modifier.size(ButtonDefaults.IconSize))
            Spacer(Modifier.size(ButtonDefaults.IconSpacing))
            Text("Save all for later")
        }
    }
    Spacer(Modifier.height(4.dp))
}
