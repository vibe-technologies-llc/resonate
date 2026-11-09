package org.resonate.android.ui.library

import androidx.annotation.StringRes
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.calculateEndPadding
import androidx.compose.foundation.layout.calculateStartPadding
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.GridItemSpan
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.lazy.grid.rememberLazyGridState
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.Sort
import androidx.compose.material.icons.rounded.ArrowDropDown
import androidx.compose.material.icons.rounded.Favorite
import androidx.compose.material.icons.rounded.Groups
import androidx.compose.material3.AssistChip
import androidx.compose.material3.AssistChipDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.resonate.android.R
import org.resonate.android.TogetherWords
import org.resonate.android.nativecore.CollectionOrder
import org.resonate.android.ui.LocalResonate
import org.resonate.android.ui.components.ArtistPortrait
import org.resonate.android.ui.components.CollectionRow
import org.resonate.android.ui.components.SectionHeader
import org.resonate.android.ui.components.Shortcut
import org.resonate.android.ui.components.ShortcutPair
import org.resonate.android.ui.components.Sizes
import org.resonate.android.ui.components.bleeding
import org.resonate.android.ui.navigation.LocalNavigator
import org.resonate.android.ui.navigation.Screen
import org.resonate.android.ui.together.rememberPasteALink
import org.resonate.android.words

@get:StringRes
val CollectionOrder.label: Int
    get() =
        when (this) {
            CollectionOrder.ALPHABETICAL -> R.string.library_order_name
            CollectionOrder.ARTIST -> R.string.library_order_artist
            CollectionOrder.YEAR -> R.string.library_order_year
            CollectionOrder.ADDED -> R.string.library_order_recently_added
            CollectionOrder.PLAYS -> R.string.library_order_most_played
        }

private val artistOrders = listOf(CollectionOrder.ALPHABETICAL, CollectionOrder.ADDED, CollectionOrder.PLAYS)

@Composable
private fun SortHeader(
    title: String,
    order: CollectionOrder,
    orders: List<CollectionOrder>,
    onOrder: (CollectionOrder) -> Unit,
    inset: Dp = Sizes.gutter,
) {
    var open by remember { mutableStateOf(false) }
    Row(
        Modifier.fillMaxWidth().padding(start = inset, end = inset, top = 8.dp, bottom = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            title,
            style = MaterialTheme.typography.titleMedium,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.weight(1f),
        )
        Box {
            AssistChip(
                onClick = { open = true },
                label = { Text(stringResource(order.label), maxLines = 1) },
                leadingIcon = { Icon(Icons.AutoMirrored.Rounded.Sort, null, Modifier.size(AssistChipDefaults.IconSize)) },
                trailingIcon = { Icon(Icons.Rounded.ArrowDropDown, null, Modifier.size(AssistChipDefaults.IconSize)) },
                modifier = Modifier.testTag("sort"),
            )
            GroupedMenu(
                open,
                { open = false },
                listOf(
                    orders.map { choice ->
                        MenuEntry.Choice(stringResource(choice.label), choice == order) { onOrder(choice) }
                    }
                ),
            )
        }
    }
}

@Composable
fun AlbumsTab(model: LibraryViewModel, padding: PaddingValues) {
    val navigator = LocalNavigator.current
    val albums by model.albums.collectAsStateWithLifecycle()
    val order by model.albumOrder.collectAsStateWithLifecycle()
    val direction = LocalLayoutDirection.current
    val grid = rememberLazyGridState()
    LoadedContent(albums, model.albumsRetry::again, Modifier.padding(padding)) { shown ->
        LazyVerticalGrid(
            columns = GridCells.Adaptive(150.dp),
            state = grid,
            contentPadding =
                PaddingValues(
                    start = padding.calculateStartPadding(direction) + Sizes.gutter,
                    end = padding.calculateEndPadding(direction) + Sizes.gutter,
                    bottom = padding.calculateBottomPadding(),
                ),
            horizontalArrangement = Arrangement.spacedBy(12.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
            modifier = Modifier.fillMaxSize().testTag("albums"),
        ) {
            item("header", span = { GridItemSpan(maxLineSpan) }) {
                SortHeader(
                    pluralStringResource(R.plurals.count_albums, shown.size, shown.size),
                    order,
                    CollectionOrder.entries,
                    { model.albumOrder.value = it },
                    inset = 0.dp,
                )
            }
            items(shown, key = { it.key }) { album ->
                AlbumCard(album, onClick = { navigator.open(album.screen) })
            }
        }
    }
}

@Composable
fun ArtistsTab(model: LibraryViewModel, padding: PaddingValues) {
    val navigator = LocalNavigator.current
    val artists by model.artists.collectAsStateWithLifecycle()
    val order by model.artistOrder.collectAsStateWithLifecycle()
    val list = rememberLazyListState()
    LoadedContent(artists, model.artistsRetry::again, Modifier.padding(padding)) { shown ->
        Box(Modifier.fillMaxSize()) {
            LazyColumn(state = list, contentPadding = padding, modifier = Modifier.fillMaxSize().testTag("artists")) {
                item("header") {
                    SortHeader(
                        pluralStringResource(R.plurals.count_artists, shown.size, shown.size),
                        order,
                        artistOrders,
                        { model.artistOrder.value = it },
                    )
                }
                items(shown, key = { "artist:${it.name}" }) { artist ->
                    CollectionRow(
                        title = artist.shownName,
                        supporting = artist.summary,
                        onClick = { navigator.open(artist.screen) },
                        leading = { ArtistPortrait(artist.artworkTrack, Sizes.rowArt) },
                    )
                }
            }
            FastScroller(list, shown.size, Modifier.align(Alignment.TopEnd).padding(padding))
        }
    }
}

@Composable
fun PlaylistsTab(model: LibraryViewModel, padding: PaddingValues) {
    val resonate = LocalResonate.current
    val navigator = LocalNavigator.current
    val playlists by model.playlists.collectAsStateWithLifecycle()
    val favourites by model.favourites.collectAsStateWithLifecycle()
    val sharedLists by model.sharedLists.collectAsStateWithLifecycle()
    val party by resonate.features.together.party.collectAsStateWithLifecycle()
    val paste = rememberPasteALink()
    val direction = LocalLayoutDirection.current
    val grid = rememberLazyGridState()
    LaunchedEffect(Unit) { model.refreshPlaylists() }
    LazyVerticalGrid(
        columns = GridCells.Adaptive(150.dp),
        state = grid,
        contentPadding =
            PaddingValues(
                start = padding.calculateStartPadding(direction) + Sizes.gutter,
                end = padding.calculateEndPadding(direction) + Sizes.gutter,
                top = 12.dp,
                bottom = padding.calculateBottomPadding(),
            ),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
        modifier = Modifier.fillMaxSize().testTag("playlists"),
    ) {
        item("shortcuts", span = { GridItemSpan(maxLineSpan) }) {
            ShortcutPair(
                first = { modifier ->
                    Shortcut(
                        icon = Icons.Rounded.Favorite,
                        title = stringResource(R.string.library_favourites),
                        detail = pluralStringResource(R.plurals.count_songs, favourites, favourites),
                        onClick = { navigator.open(Screen.Favourites) },
                        modifier = modifier.testTag("favourites"),
                    )
                },
                second = { modifier ->
                    Shortcut(
                        icon = Icons.Rounded.Groups,
                        title = stringResource(R.string.library_party),
                        detail =
                            party?.let { TogetherWords.partyLine(words(), it) }
                                ?: stringResource(R.string.library_listen_together),
                        onClick = { navigator.open(Screen.Party) },
                        modifier = modifier.testTag("listening-party"),
                    )
                },
            )
        }
        item("heading", span = { GridItemSpan(maxLineSpan) }) {
            SectionHeader(
                stringResource(R.string.library_your_playlists),
                action = stringResource(R.string.library_paste_a_link),
                onAction = paste,
                modifier = Modifier.bleeding(Sizes.gutter).testTag("playlists-heading"),
            )
        }
        if (playlists.isEmpty()) {
            item("none", span = { GridItemSpan(maxLineSpan) }) {
                EmptyCollection(
                    title = stringResource(R.string.library_no_playlists),
                    detail = stringResource(R.string.library_playlist_invitation),
                    modifier = Modifier.testTag("no-playlists"),
                )
            }
        }
        items(playlists, key = { "playlist:${it.id}" }) { playlist ->
            PlaylistCard(
                playlist,
                onClick = { navigator.open(playlist.screen) },
                badge =
                    TogetherWords.badge(
                        words(),
                        sharedLists.firstOrNull { it.playlist == playlist.id },
                        playlist.followed,
                    ),
            )
        }
    }
}
