package org.resonate.android.ui.search

import androidx.annotation.StringRes
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
import androidx.compose.material.icons.rounded.Sync
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
import androidx.compose.material3.OutlinedCard
import androidx.compose.material3.TextButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.resonate.android.LinkedPlaylistWords
import org.resonate.android.R
import org.resonate.android.Words
import org.resonate.android.nativecore.SearchAlbum
import org.resonate.android.nativecore.SearchArtist
import org.resonate.android.nativecore.SearchRelease
import org.resonate.android.nativecore.SearchSong
import org.resonate.android.nativecore.playlistLinkService
import org.resonate.android.nativecore.profileLinkService
import org.resonate.android.ui.components.EmptyState
import org.resonate.android.ui.components.IconTile
import org.resonate.android.ui.components.LocalSheets
import org.resonate.android.ui.components.SectionHeader
import org.resonate.android.ui.components.Sizes
import org.resonate.android.ui.navigation.LocalNavigator
import org.resonate.android.ui.navigation.Screen
import org.resonate.android.words

@Stable
class OnlineScroll {
    var current by mutableStateOf<LazyListState?>(null)

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
    val online by model.online.collectAsStateWithLifecycle()
    val following by model.followingProfiles.collectAsStateWithLifecycle()
    val sheets = LocalSheets.current
    val words = words()
    val levels = remember { mutableMapOf<List<OnlineView>, LazyListState>() }
    levels.keys.retainAll { place -> trail.views.take(place.size) == place }
    val profileLink = (trail.top as? OnlineView.Link)?.text
    val profileService = remember(profileLink) { profileLink?.let(::profileLinkService) }
    Box(Modifier.fillMaxSize(), contentAlignment = Alignment.TopCenter) {
        key(trail.views) {
            val state = levels.getOrPut(trail.views) { LazyListState() }
            SideEffect { scroll.current = state }
            LazyColumn(
                state = state,
                contentPadding = padding,
                modifier = Modifier.widthIn(max = ContentWidth).fillMaxSize().testTag("online-results"),
            ) {
                item(key = "download") { DownloadCard(model.downloads) }
                val top = trail.top
                if (trail.nested) item(key = "heading") { Heading(top, model::back) }
                val current = shown
                if (profileLink != null && profileService != null) {
                    if (!online) item(key = "online-off") { OnlineOffCard(model::turnOnline) }
                    else
                        item(key = "profile") {
                            ProfileCard(
                                service = profileService,
                                link = profileLink,
                                following = profileLink in following,
                                onFollow = { model.followProfile(profileLink, keptInStep = true) },
                                onMove = { model.followProfile(profileLink, keptInStep = false) },
                            )
                        }
                } else if (!online && current !is OnlineShown.Ready) {
                    item(key = "online-off") { OnlineOffCard(model::turnOnline) }
                } else when (current) {
                    OnlineShown.Idle -> loading()
                    OnlineShown.Loading -> loading()
                    is OnlineShown.Failed ->
                        item(key = "failed") {
                            EmptyState(
                                Icons.Rounded.CloudOff,
                                stringResource(R.string.online_unreached),
                                Modifier.fillParentMaxHeight(0.6f),
                                detail =
                                    current.reason
                                        ?: stringResource(R.string.online_unreached_detail),
                                action = stringResource(R.string.online_try_again),
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
                                sheets.name(words.string(R.string.online_import_title), name) {
                                    model.import(it, songs)
                                }
                            },
                            importLinked = { link, name, songs, keptInStep ->
                                sheets.name(LinkedPlaylistWords.naming(words, keptInStep), name) {
                                    model.importLinked(link, it, songs, keptInStep)
                                }
                            },
                        )
                }
            }
        }
    }
}

@Composable
private fun ProfileCard(
    service: String,
    link: String,
    following: Boolean,
    onFollow: () -> Unit,
    onMove: () -> Unit,
) {
    OutlinedCard(
        Modifier.fillMaxWidth().padding(horizontal = Sizes.gutter, vertical = 8.dp).testTag("profile-card"),
        shape = MaterialTheme.shapes.large,
    ) {
        ListItem(
            colors = ListItemDefaults.colors(containerColor = Color.Transparent),
            leadingContent = { Icon(Icons.Rounded.Person, contentDescription = null) },
            supportingContent = {
                Text(stringResource(R.string.online_profile_detail))
            },
            overlineContent = { Text(link, maxLines = 1, overflow = TextOverflow.Ellipsis) },
        ) {
            Text(
                stringResource(R.string.online_profile, service),
                modifier = Modifier.semantics { heading() },
            )
        }
        FlowRow(
            Modifier.fillMaxWidth().padding(start = 16.dp, end = 16.dp, bottom = 16.dp),
            horizontalArrangement = Arrangement.spacedBy(12.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            if (following) {
                val reading = stringResource(R.string.online_profile_reading, service)
                LoadingIndicator(Modifier.size(40.dp).semantics { contentDescription = reading })
            } else {
                Button(onClick = onFollow, modifier = Modifier.testTag("follow-profile")) {
                    Icon(Icons.Rounded.Sync, null, Modifier.size(ButtonDefaults.IconSize))
                    Spacer(Modifier.size(ButtonDefaults.IconSpacing))
                    Text(stringResource(R.string.online_profile_follow, service))
                }
                FilledTonalButton(onClick = onMove, modifier = Modifier.testTag("move-profile")) {
                    Icon(Icons.AutoMirrored.Rounded.PlaylistAdd, null, Modifier.size(ButtonDefaults.IconSize))
                    Spacer(Modifier.size(ButtonDefaults.IconSpacing))
                    Text(stringResource(R.string.online_profile_move))
                }
            }
        }
    }
}

@Composable
private fun Heading(view: OnlineView, back: () -> Boolean) {
    val words = words()
    val backWord = stringResource(R.string.online_back)
    val (title, detail, backLabel) =
        when (view) {
            OnlineView.Top -> Triple(stringResource(R.string.online_heading), "", backWord)
            is OnlineView.Found -> Triple(stringResource(view.kind.label), "", backWord)
            is OnlineView.Discography ->
                Triple(view.name, stringResource(R.string.online_discography_detail), backWord)
            is OnlineView.Editions -> Triple(view.title, editionsDetail(words, view), backWord)
            is OnlineView.Release ->
                Triple(view.title, view.detail, stringResource(R.string.online_back_to_editions))
            is OnlineView.Link ->
                Triple(
                    stringResource(R.string.online_shared_link),
                    view.text,
                    stringResource(R.string.online_close_link),
                )
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
                title.ifBlank { stringResource(R.string.search_untitled) },
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

private fun editionsDetail(words: Words, view: OnlineView.Editions): String =
    listOf(view.artist, words.string(R.string.online_editions))
        .filter { it.isNotBlank() }
        .joinToString(" · ")

private fun releaseDetail(words: Words, release: SearchRelease): String =
    listOfNotNull(
            release.date.takeIf { it.isNotBlank() },
            release.country.takeIf { it.isNotBlank() },
            release.songs
                .takeIf { it > 0u }
                ?.let { words.plural(R.plurals.count_songs, it.toInt(), it.toInt()) },
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
    importLinked: (String, String, List<SearchSong>, Boolean) -> Unit,
) {
    when (answer) {
        is OnlineAnswer.Songs -> {
            if (answer.songs.isEmpty()) {
                nothing(
                    if (top is OnlineView.Release) Icons.Rounded.MusicOff else Icons.Rounded.SearchOff,
                    if (top is OnlineView.Release) R.string.online_no_songs_listed
                    else R.string.online_no_songs,
                    if (top is OnlineView.Release) R.string.online_no_songs_listed_detail
                    else R.string.online_try_title_or_artist,
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
                nothing(
                    Icons.Rounded.Album,
                    R.string.online_no_albums,
                    R.string.online_try_title_or_artist,
                )
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
                                Text(stringResource(R.string.online_more_albums))
                            }
                        }
                    }
                }
            }
        }
        is OnlineAnswer.Artists -> {
            if (answer.artists.isEmpty()) {
                nothing(
                    Icons.Rounded.Person,
                    R.string.online_no_artists,
                    R.string.online_try_spelling,
                )
                return
            }
            artists(answer.artists, model)
        }
        is OnlineAnswer.Editions -> {
            if (answer.releases.isEmpty()) {
                nothing(
                    Icons.Rounded.Album,
                    R.string.online_no_editions,
                    R.string.online_no_editions_detail,
                )
                return
            }
            releases(answer.releases, model)
        }
        is OnlineAnswer.Link -> {
            if (answer.songs.isEmpty() && answer.releases.isEmpty() && answer.artists.isEmpty()) {
                nothing(
                    Icons.Rounded.LinkOff,
                    R.string.online_link_empty,
                    R.string.online_link_empty_detail,
                )
                return
            }
            item(key = "link-title") {
                Column(Modifier.fillMaxWidth().padding(horizontal = Sizes.gutter, vertical = 8.dp)) {
                    Text(
                        answer.name.ifBlank { stringResource(R.string.online_shared_music) },
                        style = MaterialTheme.typography.headlineSmall,
                        maxLines = 3,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.semantics { heading() },
                    )
                    if (answer.songs.isNotEmpty()) {
                        Text(
                            pluralStringResource(
                                R.plurals.count_songs,
                                answer.songs.size,
                                answer.songs.size,
                            ),
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
                        importable =
                            answer.name.ifBlank {
                                stringResource(R.string.online_imported_playlist)
                            },
                        save = model::saveAll,
                        import = import,
                        link = (top as? OnlineView.Link)?.text,
                        importLinked = importLinked,
                    )
                }
                item(key = "link-songs") { SectionHeader(stringResource(R.string.search_songs)) }
                songs(answer.songs, numbered = false, act = act)
            }
            if (answer.releases.isNotEmpty()) {
                item(key = "link-releases") {
                    SectionHeader(stringResource(R.string.online_editions))
                }
                releases(answer.releases, model)
            }
            if (answer.artists.isNotEmpty()) {
                item(key = "link-artists") {
                    SectionHeader(stringResource(R.string.search_artists))
                }
                artists(answer.artists, model)
            }
        }
    }
}

private fun LazyListScope.nothing(
    icon: ImageVector,
    @StringRes title: Int,
    @StringRes detail: Int,
) {
    item(key = "nothing") {
        EmptyState(
            icon,
            stringResource(title),
            Modifier.fillParentMaxHeight(0.55f),
            detail = stringResource(detail),
        )
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
        val detail = releaseDetail(words(), release)
        OnlineCollectionRow(
            title = release.title,
            detail = detail,
            icon = Icons.Rounded.Album,
            onClick = { model.open(OnlineView.Release(release.id, release.title, detail)) },
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
    link: String? = null,
    importLinked: (String, String, List<SearchSong>, Boolean) -> Unit = { _, _, _, _ -> },
) {
    val service = remember(link) { link?.let(::playlistLinkService) }
    FlowRow(
        Modifier.fillMaxWidth().padding(horizontal = Sizes.gutter, vertical = 8.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        if (importable != null && link != null && service != null) {
            FilledTonalButton(
                onClick = { importLinked(link, importable, songs, false) },
                modifier = Modifier.testTag("import-playlist-once"),
            ) {
                Icon(Icons.AutoMirrored.Rounded.PlaylistAdd, null, Modifier.size(ButtonDefaults.IconSize))
                Spacer(Modifier.size(ButtonDefaults.IconSpacing))
                Text(stringResource(R.string.online_import_once))
            }
            Button(
                onClick = { importLinked(link, importable, songs, true) },
                modifier = Modifier.testTag("import-playlist-in-step"),
            ) {
                Icon(Icons.Rounded.Sync, null, Modifier.size(ButtonDefaults.IconSize))
                Spacer(Modifier.size(ButtonDefaults.IconSpacing))
                Text(stringResource(R.string.online_keep_in_step, service))
            }
        } else if (importable != null) {
            Button(onClick = { import(importable, songs) }, modifier = Modifier.testTag("import-playlist")) {
                Icon(Icons.AutoMirrored.Rounded.PlaylistAdd, null, Modifier.size(ButtonDefaults.IconSize))
                Spacer(Modifier.size(ButtonDefaults.IconSpacing))
                Text(stringResource(R.string.online_import_title))
            }
        }
        FilledTonalButton(onClick = { save(songs) }, modifier = Modifier.testTag("save-all")) {
            Icon(Icons.Outlined.BookmarkAdd, null, Modifier.size(ButtonDefaults.IconSize))
            Spacer(Modifier.size(ButtonDefaults.IconSpacing))
            Text(stringResource(R.string.online_save_all))
        }
    }
    Spacer(Modifier.height(4.dp))
}

private const val ONLINE_SONGS_SHOWN = 5
private const val ONLINE_SHELF_SHOWN = 3

fun LazyListScope.onlineTop(top: OnlineTop, model: SearchViewModel, act: (SearchSong) -> Unit) {
    item(key = "online-header") { SectionHeader(stringResource(R.string.search_online)) }
    val sections = listOf(top.songs, top.albums, top.artists)
    if (sections.all { it is OnlineSection.Failed }) {
        item(key = "online-failed") {
            val reason = (top.songs as OnlineSection.Failed).reason
            ListItem(
                colors = ListItemDefaults.colors(containerColor = Color.Transparent),
                leadingContent = { Icon(Icons.Rounded.CloudOff, contentDescription = null) },
                supportingContent = {
                    Text(reason ?: stringResource(R.string.online_unreached_detail))
                },
                trailingContent = {
                    TextButton(onClick = model::retryOnline) {
                        Text(stringResource(R.string.online_try_again))
                    }
                },
            ) {
                Text(stringResource(R.string.online_unreached))
            }
        }
        return
    }
    val songs = (top.songs as? OnlineSection.Ready)?.items.orEmpty()
    val albums = (top.albums as? OnlineSection.Ready)?.items.orEmpty()
    val artists = (top.artists as? OnlineSection.Ready)?.items.orEmpty()
    if (songs.isNotEmpty()) {
        item(key = "online-songs-header") {
            SectionHeader(
                stringResource(R.string.search_songs),
                action =
                    if (songs.size > ONLINE_SONGS_SHOWN) stringResource(R.string.search_see_all)
                    else null,
                onAction = { model.open(OnlineView.Found(OnlineKind.SONGS)) },
            )
        }
        songs(songs.take(ONLINE_SONGS_SHOWN), numbered = false, act = act)
    }
    if (albums.isNotEmpty()) {
        item(key = "online-albums-header") {
            SectionHeader(
                stringResource(R.string.search_albums),
                action =
                    if (albums.size > ONLINE_SHELF_SHOWN) stringResource(R.string.search_see_all)
                    else null,
                onAction = { model.open(OnlineView.Found(OnlineKind.ALBUMS)) },
            )
        }
        albums(albums.take(ONLINE_SHELF_SHOWN), model)
    }
    if (artists.isNotEmpty()) {
        item(key = "online-artists-header") {
            SectionHeader(
                stringResource(R.string.search_artists),
                action =
                    if (artists.size > ONLINE_SHELF_SHOWN) stringResource(R.string.search_see_all)
                    else null,
                onAction = { model.open(OnlineView.Found(OnlineKind.ARTISTS)) },
            )
        }
        artists(artists.take(ONLINE_SHELF_SHOWN), model)
    }
    val awaited =
        listOf(
                R.string.online_waiting_songs to top.songs,
                R.string.online_waiting_albums to top.albums,
                R.string.online_waiting_artists to top.artists,
            )
            .firstOrNull { it.second is OnlineSection.Loading }
    if (awaited != null) {
        item(key = "online-waiting") {
            ListItem(
                modifier = Modifier.testTag("online-waiting"),
                colors = ListItemDefaults.colors(containerColor = Color.Transparent),
                leadingContent = { LoadingIndicator(Modifier.size(24.dp)) },
            ) {
                Text(stringResource(awaited.first))
            }
        }
    } else if (songs.isEmpty() && albums.isEmpty() && artists.isEmpty()) {
        item(key = "online-nothing") {
            ListItem(
                colors = ListItemDefaults.colors(containerColor = Color.Transparent),
                leadingContent = { Icon(Icons.Rounded.SearchOff, contentDescription = null) },
                supportingContent = {
                    Text(stringResource(R.string.online_try_title_or_artist))
                },
            ) {
                Text(stringResource(R.string.online_nothing))
            }
        }
    }
}
