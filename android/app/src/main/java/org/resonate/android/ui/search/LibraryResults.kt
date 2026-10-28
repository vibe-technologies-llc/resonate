package org.resonate.android.ui.search

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.TrendingUp
import androidx.compose.material.icons.outlined.BookmarkBorder
import androidx.compose.material.icons.rounded.AutoFixHigh
import androidx.compose.material.icons.rounded.Close
import androidx.compose.material.icons.rounded.CloudOff
import androidx.compose.material.icons.rounded.Favorite
import androidx.compose.material.icons.rounded.FilterList
import androidx.compose.material.icons.rounded.HighQuality
import androidx.compose.material.icons.rounded.History
import androidx.compose.material.icons.rounded.Schedule
import androidx.compose.material.icons.rounded.Search
import androidx.compose.material.icons.rounded.SearchOff
import androidx.compose.material.icons.rounded.TravelExplore
import androidx.compose.material.icons.rounded.Explore
import androidx.compose.material3.AssistChip
import androidx.compose.material3.AssistChipDefaults
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedCard
import androidx.compose.material3.SuggestionChip
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.resonate.android.nativecore.AlbumHit
import org.resonate.android.nativecore.ArtistHit
import org.resonate.android.nativecore.LibrarySearch
import org.resonate.android.nativecore.SearchColumn
import org.resonate.android.nativecore.SearchSong
import org.resonate.android.nativecore.Track
import org.resonate.android.ui.Mix
import org.resonate.android.ui.components.ArtistPortrait
import org.resonate.android.ui.components.EmptyState
import org.resonate.android.ui.components.LocalSheets
import org.resonate.android.ui.components.RowSelection
import org.resonate.android.ui.components.SectionHeader
import org.resonate.android.ui.components.Selection
import org.resonate.android.ui.components.SongArtwork
import org.resonate.android.ui.components.Sizes
import org.resonate.android.ui.components.TrackRow
import org.resonate.android.ui.components.lit
import org.resonate.android.ui.counted
import org.resonate.android.ui.library.SavedSearchDraft
import org.resonate.android.ui.library.shownName
import org.resonate.android.ui.navigation.LocalNavigator
import org.resonate.android.ui.navigation.Screen
import org.resonate.android.ui.shownArtist
import org.resonate.android.ui.shownTitle

@Composable
fun LibraryResults(
    model: SearchViewModel,
    state: LazyListState,
    padding: PaddingValues,
    selection: Selection<ULong>,
    act: (SearchSong) -> Unit,
) {
    val shown by model.libraryShown.collectAsStateWithLifecycle()
    val chosen by model.scope.collectAsStateWithLifecycle()
    val playing by model.playing.collectAsStateWithLifecycle()
    val recent by model.recent.collectAsStateWithLifecycle()
    val online by model.online.collectAsStateWithLifecycle()
    val onlineFound by model.onlineTop.collectAsStateWithLifecycle()
    val navigator = LocalNavigator.current
    val sheets = LocalSheets.current
    val showing =
        when (val current = shown) {
            is LibraryShown.Answered -> current.answer.query
            is LibraryShown.Failed -> current.query
            LibraryShown.Asking -> null
            LibraryShown.Idle -> ""
        }
    var lastShowing by rememberSaveable { mutableStateOf<String?>(null) }
    LaunchedEffect(showing) {
        if (showing != null && showing != lastShowing) {
            if (lastShowing != null) state.scrollToItem(0)
            lastShowing = showing
        }
    }
    BoxWithConstraints(Modifier.fillMaxSize(), contentAlignment = Alignment.TopCenter) {
        val width = minOf(maxWidth, ContentWidth)
        val columns = columnsFor(width)
        LazyColumn(
            state = state,
            contentPadding = padding,
            modifier = Modifier.widthIn(max = ContentWidth).fillMaxSize().testTag("library-results"),
        ) {
            item(key = "download") { DownloadCard(model.downloads) }
            when (val current = shown) {
                LibraryShown.Idle ->
                    browse(
                        recent = recent,
                        online = online,
                        columns = columns,
                        search = model::search,
                        forget = model::forget,
                        forgetAll = model::forgetAll,
                        turnOnline = model::turnOnline,
                        open = navigator::open,
                    )
                LibraryShown.Asking -> loading()
                is LibraryShown.Failed ->
                    item(key = "failed") {
                        EmptyState(
                            Icons.Rounded.SearchOff,
                            "Search could not finish",
                            Modifier.fillParentMaxHeight(0.7f),
                            detail = "Something went wrong while searching your library.",
                            action = "Try again",
                            onAction = model::retryLibrary,
                        )
                    }
                is LibraryShown.Answered ->
                    answered(
                        answer = current.answer,
                        chosen = chosen,
                        playing = playing,
                        online = online,
                        onlineFound = onlineFound,
                        act = act,
                        columns = columns,
                        model = model,
                        openAlbum = { album ->
                            model.keep()
                            navigator.open(
                                Screen.Album(album.key, album.title, album.artist, album.artworkTrack)
                            )
                        },
                        openArtist = { artist ->
                            model.keep()
                            navigator.open(Screen.Artist(artist.name, artist.artworkTrack))
                        },
                        more = { track -> sheets.song(track) },
                        selection = selection,
                    )
            }
        }
    }
}

private fun columnsFor(width: Dp): Int {
    val usable = width - Sizes.gutter * 2 + TILE_SPACING
    return (usable / (TILE_AT_LEAST + TILE_SPACING)).toInt().coerceIn(2, 6)
}

fun LazyListScope.loading() {
    item(key = "loading") {
        Box(Modifier.fillMaxWidth().padding(vertical = 64.dp), contentAlignment = Alignment.Center) {
            LoadingIndicator(Modifier.size(56.dp))
        }
    }
}

private class BrowseTile(val title: String, val icon: ImageVector, val screen: Screen)

private val browseTiles =
    listOf(
        BrowseTile("Favourites", Icons.Rounded.Favorite, Screen.Favourites),
        BrowseTile(Mix.RECENTLY_ADDED.title, Icons.Rounded.Schedule, Screen.Mix(Mix.RECENTLY_ADDED)),
        BrowseTile(Mix.MOST_PLAYED.title, Icons.AutoMirrored.Rounded.TrendingUp, Screen.Mix(Mix.MOST_PLAYED)),
        BrowseTile(Mix.NEVER_HEARD.title, Icons.Rounded.Explore, Screen.Mix(Mix.NEVER_HEARD)),
        BrowseTile(Mix.HIGH_RESOLUTION.title, Icons.Rounded.HighQuality, Screen.Mix(Mix.HIGH_RESOLUTION)),
        BrowseTile("Saved for later", Icons.Outlined.BookmarkBorder, Screen.Wanted),
    )

private class SearchTip(val label: String, val query: String)

private val searchTips =
    listOf(
        SearchTip("Lossless", "is:lossless"),
        SearchTip("Hi-res", "is:hires"),
        SearchTip("Lossy", "is:lossy"),
        SearchTip("Favourites", "is:favourite"),
        SearchTip("Mono", "is:mono"),
        SearchTip("Multichannel", "is:multichannel"),
    )

fun LazyListScope.recentSearches(
    recent: List<String>,
    search: (String) -> Unit,
    forget: (String) -> Unit,
    forgetAll: () -> Unit,
) {
    if (recent.isEmpty()) return
    item(key = "recent-header") { SectionHeader("Recent searches", action = "Clear", onAction = forgetAll) }
    items(recent, key = { "recent-$it" }) { text ->
        ListItem(
            modifier = Modifier.animateItem().clickable { search(text) }.testTag("recent-search"),
            colors = ListItemDefaults.colors(containerColor = Color.Transparent),
            leadingContent = { Icon(Icons.Rounded.History, contentDescription = null) },
            trailingContent = {
                IconButton(onClick = { forget(text) }) {
                    Icon(Icons.Rounded.Close, contentDescription = "Remove “$text” from recent searches")
                }
            },
        ) { Text(text, maxLines = 1, overflow = TextOverflow.Ellipsis) }
    }
}

@OptIn(ExperimentalLayoutApi::class)
private fun LazyListScope.browse(
    recent: List<String>,
    online: Boolean,
    columns: Int,
    search: (String) -> Unit,
    forget: (String) -> Unit,
    forgetAll: () -> Unit,
    turnOnline: () -> Unit,
    open: (Screen) -> Unit,
) {
    if (!online) item(key = "online-off") { OnlineOffCard(turnOnline) }
    recentSearches(recent, search, forget, forgetAll)
    item(key = "browse-header") { SectionHeader("Browse your library") }
    val tileColumns = (columns).coerceAtMost(3).coerceAtLeast(2)
    browseTiles.chunked(tileColumns).forEachIndexed { row, tiles ->
        item(key = "browse-$row") {
            Row(
                Modifier.fillMaxWidth().padding(horizontal = Sizes.gutter, vertical = 4.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                tiles.forEach { tile ->
                    BrowseCard(tile, Modifier.weight(1f)) { open(tile.screen) }
                }
                repeat(tileColumns - tiles.size) { Spacer(Modifier.weight(1f)) }
            }
        }
    }
    item(key = "tips-header") { SectionHeader("Search by sound") }
    item(key = "tips") {
        FlowRow(
            Modifier.fillMaxWidth().padding(horizontal = Sizes.gutter),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            searchTips.forEach { tip ->
                SuggestionChip(
                    onClick = { search(tip.query) },
                    label = { Text(tip.label) },
                    icon = {
                        Icon(
                            Icons.Rounded.Search,
                            contentDescription = null,
                            modifier = Modifier.size(AssistChipDefaults.IconSize),
                        )
                    },
                    modifier = Modifier.semantics { contentDescription = "Search for ${tip.query}" },
                )
            }
        }
        Text(
            "Words match in any order. Try artist:, album:, year: or genre: to narrow a search.",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(horizontal = Sizes.gutter, vertical = 12.dp),
        )
    }
}

@Composable
private fun BrowseCard(tile: BrowseTile, modifier: Modifier, onClick: () -> Unit) {
    Card(
        onClick = onClick,
        modifier = modifier.testTag("browse-tile"),
        shape = MaterialTheme.shapes.large,
        colors =
            CardDefaults.cardColors(
                containerColor = MaterialTheme.colorScheme.secondaryContainer,
                contentColor = MaterialTheme.colorScheme.onSecondaryContainer,
            ),
    ) {
        Row(
            Modifier.fillMaxWidth().padding(horizontal = 14.dp, vertical = 12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(
                tile.icon,
                contentDescription = null,
                tint = MaterialTheme.colorScheme.primary,
                modifier = Modifier.size(22.dp),
            )
            Spacer(Modifier.width(10.dp))
            Text(tile.title, style = MaterialTheme.typography.titleSmall, maxLines = 2, overflow = TextOverflow.Ellipsis)
        }
    }
}

@Composable
fun OnlineOffCard(turnOnline: () -> Unit) {
    OutlinedCard(
        Modifier.fillMaxWidth().padding(horizontal = Sizes.gutter, vertical = 8.dp).testTag("online-off"),
        shape = MaterialTheme.shapes.large,
    ) {
        ListItem(
            colors = ListItemDefaults.colors(containerColor = Color.Transparent),
            leadingContent = { Icon(Icons.Rounded.CloudOff, contentDescription = null) },
            supportingContent = {
                Text("Turn on online features to find music to download, artwork and lyrics.")
            },
        ) { Text("Online search is off") }
        Row(
            Modifier.fillMaxWidth().padding(start = 16.dp, end = 16.dp, bottom = 12.dp),
            horizontalArrangement = Arrangement.End,
        ) {
            FilledTonalButton(onClick = turnOnline) { Text("Turn on") }
        }
    }
}

private fun LazyListScope.answered(
    answer: LibraryAnswer,
    chosen: LibraryScope,
    playing: ULong?,
    online: Boolean,
    onlineFound: OnlineTop?,
    act: (SearchSong) -> Unit,
    columns: Int,
    model: SearchViewModel,
    openAlbum: (AlbumHit) -> Unit,
    openArtist: (ArtistHit) -> Unit,
    more: (Track) -> Unit,
    selection: Selection<ULong>,
) {
    val found = answer.found
    val scope = shownScope(chosen, found)
    hints(answer, model::search)
    if (matchedNothing(found)) {
        item(key = "nothing") {
            ListItem(
                colors = ListItemDefaults.colors(containerColor = Color.Transparent),
                leadingContent = { Icon(Icons.Rounded.SearchOff, contentDescription = null) },
                supportingContent = { Text("Check the spelling, or try fewer words.") },
                modifier = Modifier.testTag("nothing-in-library"),
            ) {
                Text("No matches for “${answer.query}” in your library", maxLines = 2)
            }
        }
        onlineSection(online, onlineFound, model, act)
        return
    }
    item(key = "scopes") { ScopeChips(found, scope, model::choose) }
    when (scope) {
        LibraryScope.TOP -> {
            songs(answer, found.tracks.take(TOP_SONGS), found.tracks, playing, model, more, selection)
            if (found.tracks.size > TOP_SONGS) {
                item(key = "songs-all") {
                    SeeAll("See all ${songsCounted(found)} songs") { model.choose(LibraryScope.SONGS) }
                }
            }
            if (found.albums.isNotEmpty()) {
                item(key = "albums-header") {
                    SectionHeader(
                        "Albums",
                        action = if (found.albums.size > TOP_SHELF) "See all" else null,
                        onAction = { model.choose(LibraryScope.ALBUMS) },
                    )
                }
                item(key = "albums-shelf") {
                    LazyRow(
                        contentPadding = PaddingValues(horizontal = Sizes.gutter),
                        horizontalArrangement = Arrangement.spacedBy(TILE_SPACING),
                        modifier = Modifier.testTag("album-shelf"),
                    ) {
                        items(found.albums.take(TOP_SHELF), key = { it.key }) { album ->
                            AlbumTile(album, answer.lit, Sizes.carouselCover) { openAlbum(album) }
                        }
                    }
                }
            }
            if (found.artists.isNotEmpty()) {
                item(key = "artists-header") {
                    SectionHeader(
                        "Artists",
                        action = if (found.artists.size > TOP_SHELF) "See all" else null,
                        onAction = { model.choose(LibraryScope.ARTISTS) },
                    )
                }
                item(key = "artists-shelf") {
                    LazyRow(
                        contentPadding = PaddingValues(horizontal = Sizes.gutter),
                        horizontalArrangement = Arrangement.spacedBy(TILE_SPACING),
                        modifier = Modifier.testTag("artist-shelf"),
                    ) {
                        items(found.artists.take(TOP_SHELF), key = { it.name }) { artist ->
                            ArtistTile(artist, answer.lit) { openArtist(artist) }
                        }
                    }
                }
            }
            onlineSection(online, onlineFound, model, act)
        }
        LibraryScope.SONGS -> {
            item(key = "save-search") {
                val sheets = LocalSheets.current
                Row(Modifier.fillMaxWidth().padding(horizontal = Sizes.gutter), horizontalArrangement = Arrangement.End) {
                    TextButton(
                        onClick = {
                            sheets.editSearch(
                                "Save search as playlist",
                                SavedSearchDraft.starting(answer.query, answer.query),
                            ) { name, search ->
                                model.saveSearch(name, search)
                            }
                        },
                        modifier = Modifier.testTag("save-search"),
                    ) {
                        Text("Save as playlist")
                    }
                }
            }
            songs(answer, found.tracks, found.tracks, playing, model, more, selection)
            if (found.trackCount > found.tracks.size.toUInt()) {
                item(key = "songs-capped") {
                    Column(Modifier.padding(horizontal = Sizes.gutter, vertical = 16.dp)) {
                        Text(
                            "Showing ${found.tracks.size} of ${found.trackCount} songs.",
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                        val loadingMore by model.loadingMoreSongs.collectAsStateWithLifecycle()
                        if (loadingMore) {
                            LoadingIndicator(Modifier.size(40.dp).testTag("more-songs-loading"))
                        } else {
                            TextButton(onClick = model::moreSongs, modifier = Modifier.testTag("more-songs")) {
                                Text("Show more")
                            }
                        }
                    }
                }
            }
        }
        LibraryScope.ALBUMS ->
            found.albums.chunked(columns).forEachIndexed { row, albums ->
                item(key = "albums-$row") {
                    BoxWithConstraints(Modifier.fillMaxWidth().padding(horizontal = Sizes.gutter)) {
                        val tile = (maxWidth - TILE_SPACING * (columns - 1)) / columns
                        Row(
                            Modifier.padding(vertical = 8.dp),
                            horizontalArrangement = Arrangement.spacedBy(TILE_SPACING),
                        ) {
                            albums.forEach { album ->
                                AlbumTile(album, answer.lit, tile) { openAlbum(album) }
                            }
                        }
                    }
                }
            }
        LibraryScope.ARTISTS ->
            items(found.artists, key = { "artist-${it.name}" }) { artist ->
                ListItem(
                    modifier = Modifier.clickable { openArtist(artist) },
                    colors = ListItemDefaults.colors(containerColor = Color.Transparent),
                    leadingContent = { ArtistPortrait(artist.artworkTrack, Sizes.rowArt) },
                    supportingContent = {
                        Text(
                            "${counted(artist.albums, "album")} · ${counted(artist.songs, "song")}",
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                    },
                ) {
                        Text(
                            lit(artist.shownName, answer.lit.of(SearchColumn.ARTIST, artist.name), bold()),
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                    }
            }
    }
}

private fun LazyListScope.onlineSection(
    online: Boolean,
    onlineFound: OnlineTop?,
    model: SearchViewModel,
    act: (SearchSong) -> Unit,
) {
    when {
        !online -> {
            item(key = "online-header") { SectionHeader("Online") }
            item(key = "online-off") { OnlineOffCard(model::turnOnline) }
        }
        onlineFound != null -> onlineTop(onlineFound, model, act)
    }
}

private fun LazyListScope.hints(answer: LibraryAnswer, search: (String) -> Unit) {
    val found = answer.found
    if (found.reads.isNotEmpty()) item(key = "reads") { Reads(found.reads) }
    found.didYouMean
        ?.takeIf { it.isNotBlank() }
        ?.let { meant ->
            item(key = "meant") {
                AssistChip(
                    onClick = { search(meant) },
                    label = {
                        Text(
                            buildAnnotatedString {
                                append("Did you mean ")
                                withStyle(SpanStyle(fontWeight = FontWeight.Bold)) { append(meant) }
                                append("?")
                            }
                        )
                    },
                    leadingIcon = {
                        Icon(
                            Icons.Rounded.AutoFixHigh,
                            contentDescription = null,
                            tint = MaterialTheme.colorScheme.primary,
                            modifier = Modifier.size(AssistChipDefaults.IconSize),
                        )
                    },
                    modifier = Modifier.padding(horizontal = Sizes.gutter).testTag("did-you-mean"),
                )
            }
        }
    if (answer.completions.isNotEmpty()) {
        item(key = "completions") {
            LazyRow(
                contentPadding = PaddingValues(horizontal = Sizes.gutter),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
                modifier = Modifier.testTag("completions"),
            ) {
                items(answer.completions, key = { it }) { completion ->
                    SuggestionChip(
                        onClick = { search(completion) },
                        label = { Text(completion, maxLines = 1) },
                        icon = {
                            Icon(
                                Icons.Rounded.Search,
                                contentDescription = null,
                                modifier = Modifier.size(AssistChipDefaults.IconSize),
                            )
                        },
                    )
                }
            }
        }
    }
}

@Composable
private fun Reads(reads: List<String>) {
    Row(
        Modifier.fillMaxWidth()
            .padding(horizontal = Sizes.gutter, vertical = 4.dp)
            .semantics(mergeDescendants = true) {
                contentDescription = "Searching for ${reads.joinToString(", ")}"
            },
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(
            Icons.Rounded.FilterList,
            contentDescription = null,
            tint = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.size(18.dp),
        )
        Spacer(Modifier.width(8.dp))
        LazyRow(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            items(reads) { read ->
                Surface(
                    color = MaterialTheme.colorScheme.secondaryContainer,
                    contentColor = MaterialTheme.colorScheme.onSecondaryContainer,
                    shape = MaterialTheme.shapes.small,
                ) {
                    Text(
                        read,
                        style = MaterialTheme.typography.labelLarge,
                        maxLines = 1,
                        modifier = Modifier.padding(horizontal = 10.dp, vertical = 6.dp),
                    )
                }
            }
        }
    }
}

@Composable
private fun ScopeChips(found: LibrarySearch, scope: LibraryScope, choose: (LibraryScope) -> Unit) {
    LazyRow(
        contentPadding = PaddingValues(horizontal = Sizes.gutter),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        modifier = Modifier.padding(vertical = 4.dp).testTag("search-scopes"),
    ) {
        items(scopes(found), key = { it.name }) { option ->
            FilterChip(
                selected = option == scope,
                onClick = { choose(option) },
                label = { Text(scopeLabel(option, found), maxLines = 1) },
                modifier = Modifier.testTag("scope-${option.name.lowercase()}"),
            )
        }
    }
}

private fun LazyListScope.songs(
    answer: LibraryAnswer,
    shown: List<Track>,
    all: List<Track>,
    playing: ULong?,
    model: SearchViewModel,
    more: (Track) -> Unit,
    selection: Selection<ULong>,
) {
    if (shown.isEmpty()) return
    item(key = "songs-header") { SectionHeader("Songs") }
    itemsIndexed(shown, key = { _, track -> "song-${track.id}" }) { index, track ->
        LitTrackRow(
            track = track,
            lit = answer.lit,
            playing = playing == track.id,
            onClick = { model.play(all, index) },
            onMore = { more(track) },
            selection = selection.row(track.id),
        )
    }
}

@Composable
private fun LitTrackRow(
    track: Track,
    lit: Lit,
    playing: Boolean,
    onClick: () -> Unit,
    onMore: () -> Unit,
    selection: RowSelection,
) {
    val title = bold()
    val supporting = bold(MaterialTheme.colorScheme.onSurface)
    val headline =
        remember(track, lit, title) {
            lit(track.shownTitle, lit.of(SearchColumn.TITLE, track.shownTitle), title)
        }
    val byline =
        remember(track, lit, supporting) {
            buildAnnotatedString {
                append(lit(track.shownArtist, lit.of(SearchColumn.ARTIST, track.artist), supporting))
                if (track.album.isNotBlank()) {
                    append(" · ")
                    append(lit(track.album, lit.of(SearchColumn.ALBUM, track.album), supporting))
                }
            }
        }
    TrackRow(
        track = track,
        onClick = onClick,
        playing = playing,
        title = headline,
        subtitle = byline,
        onMore = onMore,
        selection = selection,
        modifier = Modifier.testTag("search-song"),
    )
}

@Composable
private fun bold(color: Color = Color.Unspecified) =
    remember(color) { SpanStyle(fontWeight = FontWeight.Bold, color = color) }

@Composable
private fun AlbumTile(album: AlbumHit, lit: Lit, size: Dp, onClick: () -> Unit) {
    val emphasis = bold()
    val detail = bold(MaterialTheme.colorScheme.onSurface)
    Column(
        Modifier.width(size)
            .clip(MaterialTheme.shapes.large)
            .clickable(onClick = onClick)
            .testTag("search-album")
    ) {
        SongArtwork(album.artworkTrack, size, shape = MaterialTheme.shapes.large)
        Spacer(Modifier.height(8.dp))
        Text(
            lit(album.title, lit.of(SearchColumn.ALBUM, album.title), emphasis),
            style = MaterialTheme.typography.titleSmall,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.padding(horizontal = 4.dp),
        )
        Text(
            buildAnnotatedString {
                append(lit(album.artist, lit.of(SearchColumn.ARTIST, album.artist), detail))
                album.year?.let { append(" · $it") }
            },
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.padding(start = 4.dp, end = 4.dp, bottom = 8.dp),
        )
    }
}

@Composable
private fun ArtistTile(artist: ArtistHit, lit: Lit, onClick: () -> Unit) {
    val emphasis = bold()
    Column(
        Modifier.width(Sizes.artistCircle)
            .clip(MaterialTheme.shapes.large)
            .clickable(onClick = onClick)
            .testTag("search-artist"),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        ArtistPortrait(artist.artworkTrack, Sizes.artistCircle)
        Spacer(Modifier.height(8.dp))
        Text(
            lit(artist.shownName, lit.of(SearchColumn.ARTIST, artist.name), emphasis),
            style = MaterialTheme.typography.titleSmall,
            textAlign = TextAlign.Center,
            maxLines = 2,
            overflow = TextOverflow.Ellipsis,
        )
        Text(
            counted(artist.songs, "song"),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            textAlign = TextAlign.Center,
            maxLines = 1,
            modifier = Modifier.padding(bottom = 8.dp),
        )
    }
}

@Composable
private fun SeeAll(label: String, onClick: () -> Unit) {
    Box(Modifier.fillMaxWidth().padding(horizontal = 8.dp)) {
        TextButton(onClick = onClick) { Text(label) }
    }
}

private val TILE_AT_LEAST = 148.dp
val TILE_SPACING = 12.dp
