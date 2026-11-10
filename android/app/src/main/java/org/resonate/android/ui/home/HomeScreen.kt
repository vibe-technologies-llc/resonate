package org.resonate.android.ui.home

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Add
import androidx.compose.material.icons.rounded.Favorite
import androidx.compose.material.icons.rounded.Shuffle
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.VerticalDivider
import androidx.compose.material3.carousel.HorizontalMultiBrowseCarousel
import androidx.compose.material3.carousel.rememberCarouselState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.RectangleShape
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.LifecycleResumeEffect
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import java.time.LocalTime
import kotlin.math.roundToInt
import org.resonate.android.R
import org.resonate.android.nativecore.AlbumHit
import org.resonate.android.nativecore.LibraryStatistics
import org.resonate.android.nativecore.SuggestedList
import org.resonate.android.ui.LocalResonate
import org.resonate.android.ui.components.CollectionRow
import org.resonate.android.ui.components.IconTile
import org.resonate.android.ui.components.LocalSheets
import org.resonate.android.ui.components.Page
import org.resonate.android.ui.components.Picture
import org.resonate.android.ui.components.PlaylistCover
import org.resonate.android.ui.components.SectionHeader
import org.resonate.android.ui.components.SettingsAction
import org.resonate.android.ui.components.Shortcut
import org.resonate.android.ui.components.ShortcutPair
import org.resonate.android.ui.components.Sizes
import org.resonate.android.ui.components.TitleSize
import org.resonate.android.ui.components.rememberArtwork
import org.resonate.android.ui.components.rememberTitleMaxLines
import org.resonate.android.ui.icon
import org.resonate.android.ui.library.ArtistCard
import org.resonate.android.ui.library.CoverCard
import org.resonate.android.ui.library.Loading
import org.resonate.android.ui.library.PlaylistCard
import org.resonate.android.ui.library.Shelf
import org.resonate.android.ui.library.Shelves
import org.resonate.android.ui.library.Unreadable
import org.resonate.android.ui.library.Welcome
import org.resonate.android.ui.library.greeting
import org.resonate.android.ui.library.listenedHours
import org.resonate.android.ui.library.playShuffled
import org.resonate.android.ui.library.screen
import org.resonate.android.ui.library.shownName
import org.resonate.android.ui.library.shownTitle
import org.resonate.android.ui.library.valueOrNull
import org.resonate.android.ui.navigation.LocalNavigator
import org.resonate.android.ui.navigation.Screen
import org.resonate.android.ui.resonateViewModel
import org.resonate.android.ui.screen
import org.resonate.android.words

@Composable
fun HomeScreen() {
    val model = resonateViewModel { HomeViewModel(it) }
    val ready by model.ready.collectAsStateWithLifecycle()
    val songs by model.songs.collectAsStateWithLifecycle()
    val unread by model.unread.collectAsStateWithLifecycle()
    var hour by remember { mutableIntStateOf(LocalTime.now().hour) }
    val title = greeting(words(), hour)
    LifecycleResumeEffect(model) {
        hour = LocalTime.now().hour
        model.refresh()
        onPauseOrDispose {}
    }
    Page(title, size = TitleSize.LARGE, back = false, actions = { SettingsAction() }) { padding ->
        when {
            !ready -> Loading(Modifier.padding(padding))
            songs.isEmpty() && unread -> Unreadable(model::reread, Modifier.padding(padding))
            songs.isEmpty() -> Box(Modifier.fillMaxSize().padding(padding)) { Welcome() }
            else -> HomeFeed(model, padding)
        }
    }
}

@Composable
private fun HomeFeed(model: HomeViewModel, padding: PaddingValues) {
    val resonate = LocalResonate.current
    val navigator = LocalNavigator.current
    val sheets = LocalSheets.current
    val songs by model.songs.collectAsStateWithLifecycle()
    val favourites by model.favourites.collectAsStateWithLifecycle()
    val mixes by model.mixes.collectAsStateWithLifecycle()
    val albums by model.recentAlbums.collectAsStateWithLifecycle()
    val artists by model.artists.collectAsStateWithLifecycle()
    val playlists by model.playlists.collectAsStateWithLifecycle()
    val statistics by model.statistics.collectAsStateWithLifecycle()
    val newPlaylist = stringResource(R.string.library_new_playlist)
    LazyColumn(contentPadding = padding, modifier = Modifier.fillMaxSize().testTag("home")) {
        item("shortcuts") {
            ShortcutPair(
                modifier = Modifier.padding(horizontal = Sizes.gutter, vertical = 8.dp),
                first = { modifier ->
                    Shortcut(
                        icon = Icons.Rounded.Favorite,
                        title = stringResource(R.string.library_favourites),
                        detail = pluralStringResource(R.plurals.count_songs, favourites.size, favourites.size),
                        onClick = { navigator.open(Screen.Favourites) },
                        modifier = modifier.testTag("home-favourites"),
                    )
                },
                second = { modifier ->
                    Shortcut(
                        icon = Icons.Rounded.Shuffle,
                        title = stringResource(R.string.home_shuffle_all),
                        detail = pluralStringResource(R.plurals.count_songs, songs.size, songs.size),
                        onClick = { resonate.playShuffled(songs) },
                        modifier = modifier.testTag("home-shuffle"),
                    )
                },
            )
        }
        mixes.valueOrNull()?.takeIf { it.isNotEmpty() }?.let { shown ->
            item("mixes-title") { SectionHeader(stringResource(R.string.library_made_for_you)) }
            item("mixes") {
                Shelf(shown, key = { it.id }) { list ->
                    MixCard(list, onClick = { navigator.open(list.screen) })
                }
            }
        }
        albums.valueOrNull()?.takeIf { it.isNotEmpty() }?.let { shown ->
            item("recent-title") { SectionHeader(stringResource(R.string.library_order_recently_added)) }
            item("recent") { RecentAlbums(shown) { navigator.open(it.screen) } }
        }
        item("playlists-title") { SectionHeader(stringResource(R.string.library_your_playlists)) }
        if (playlists.isEmpty()) {
            item("playlists-none") {
                CollectionRow(
                    title = newPlaylist,
                    supporting = stringResource(R.string.library_playlist_invitation),
                    onClick = { sheets.name(newPlaylist) { model.createPlaylist(it) } },
                    leading = { IconTile(Icons.Rounded.Add) },
                    modifier = Modifier.testTag("home-new-playlist"),
                )
            }
        } else item("playlists") {
            Shelf(playlists + listOf(null), key = { it?.id?.toString() ?: "new" }) { playlist ->
                if (playlist == null) {
                    NewPlaylistCard { sheets.name(newPlaylist) { model.createPlaylist(it) } }
                } else {
                    PlaylistCard(
                        playlist,
                        onClick = { navigator.open(playlist.screen) },
                        modifier = Modifier.width(Shelves.playlistWidth),
                    )
                }
            }
        }
        artists.valueOrNull()?.takeIf { it.isNotEmpty() }?.let { shown ->
            item("artists-title") { SectionHeader(stringResource(R.string.library_shelf_artists)) }
            item("artists") {
                Shelf(shown, key = { "artist:${it.name}" }) { artist ->
                    ArtistCard(
                        artist.shownName,
                        artist.artworkTrack,
                        pluralStringResource(R.plurals.count_songs, artist.songs.toInt(), artist.songs.toInt()),
                        onClick = { navigator.open(artist.screen) },
                    )
                }
            }
        }
        item("listening-title") {
            SectionHeader(
                stringResource(R.string.home_your_listening),
                action = stringResource(R.string.home_statistics),
                onAction = { navigator.open(Screen.Statistics) },
            )
        }
        item("listening") {
            ListeningFigures(statistics, songs.size) { navigator.open(Screen.Statistics) }
        }
    }
}

@Composable
private fun MixCard(list: SuggestedList, onClick: () -> Unit) {
    val scheme = MaterialTheme.colorScheme
    val lines = rememberTitleMaxLines()
    CoverCard(
        title = list.name,
        detail = list.detail,
        onClick = onClick,
        modifier =
            Modifier.width(Shelves.playlistWidth)
                .testTag("mix-${list.reason.name.lowercase()}"),
        titleLines = maxOf(lines, MIX_TITLE_LINES),
        detailLines = maxOf(lines, MIX_DETAIL_LINES),
    ) {
        Box {
            PlaylistCover(list.covers, Shelves.playlistWidth, shape = MaterialTheme.shapes.large)
            Surface(
                color = scheme.surface.copy(alpha = REASON_BADGE_ALPHA),
                contentColor = scheme.onSurface,
                shape = CircleShape,
                modifier = Modifier.align(Alignment.BottomStart).padding(8.dp).size(28.dp),
            ) {
                Box(contentAlignment = Alignment.Center) {
                    Icon(
                        list.reason.icon,
                        contentDescription = null,
                        modifier = Modifier.size(16.dp),
                    )
                }
            }
        }
    }
}

@Composable
private fun RecentAlbums(albums: List<AlbumHit>, onOpen: (AlbumHit) -> Unit) {
    val state = rememberCarouselState { albums.size }
    HorizontalMultiBrowseCarousel(
        state = state,
        preferredItemWidth = 200.dp,
        itemSpacing = 8.dp,
        contentPadding = PaddingValues(horizontal = Sizes.gutter),
        modifier = Modifier.fillMaxWidth().height(220.dp).testTag("recent-albums"),
    ) { index ->
        val album = albums[index]
        val info = carouselItemDrawInfo
        val opened = if (info.maxSize > info.minSize) ((info.size - info.minSize) / (info.maxSize - info.minSize)).coerceIn(0f, 1f) else 1f
        val caption = ((opened - CAPTION_GONE_BELOW) / (CAPTION_WHOLE_FROM - CAPTION_GONE_BELOW)).coerceIn(0f, 1f)
        val described = "${album.shownTitle}, ${album.artist}"
        BoxWithConstraints(
            Modifier.fillMaxSize()
                .maskClip(MaterialTheme.shapes.extraLarge)
                .semantics(mergeDescendants = true) {
                    role = Role.Button
                    contentDescription = described
                }
                .clickable { onOpen(album) }
        ) {
            val density = LocalDensity.current
            val edge = with(density) { maxWidth.roundToPx() }
            Picture(rememberArtwork(album.artworkTrack, 0u, edge), Modifier.fillMaxSize(), RectangleShape)
            Surface(
                color = MaterialTheme.colorScheme.surfaceContainerHigh.copy(alpha = 0.92f),
                shape = MaterialTheme.shapes.medium,
                modifier =
                    Modifier.align(Alignment.BottomStart)
                        .offset { IntOffset(info.maskRect.left.roundToInt(), 0) }
                        .padding(10.dp)
                        .widthIn(max = with(density) { info.maskRect.width.toDp() } - 20.dp)
                        .graphicsLayer { alpha = caption },
            ) {
                Column(Modifier.padding(horizontal = 10.dp, vertical = 6.dp)) {
                    Text(album.shownTitle, style = MaterialTheme.typography.titleSmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    Text(
                        album.artist,
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
            }
        }
    }
}

@Composable
private fun NewPlaylistCard(onClick: () -> Unit) {
    Column(Modifier.width(Shelves.playlistWidth)) {
        Surface(
            onClick = onClick,
            color = MaterialTheme.colorScheme.surfaceContainerHigh,
            contentColor = MaterialTheme.colorScheme.primary,
            shape = MaterialTheme.shapes.large,
            modifier = Modifier.size(Shelves.playlistWidth).testTag("home-new-playlist"),
        ) {
            Box(contentAlignment = Alignment.Center) {
                Icon(Icons.Rounded.Add, contentDescription = null, modifier = Modifier.size(40.dp))
            }
        }
        Spacer(Modifier.height(8.dp))
        Text(
            stringResource(R.string.library_new_playlist),
            style = MaterialTheme.typography.titleSmall,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.padding(horizontal = 4.dp),
        )
    }
}

@Composable
private fun ListeningFigures(statistics: LibraryStatistics?, songs: Int, onClick: () -> Unit) {
    val plays = statistics?.plays?.toInt() ?: 0
    Row(
        Modifier.fillMaxWidth()
            .padding(horizontal = Sizes.gutter - FIGURES_INSET)
            .clip(MaterialTheme.shapes.large)
            .clickable(onClick = onClick, role = Role.Button)
            .semantics(mergeDescendants = true) {}
            .padding(horizontal = FIGURES_INSET, vertical = 8.dp)
            .height(IntrinsicSize.Min)
            .testTag("home-statistics"),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Figure(
            statistics?.plays?.toString() ?: NO_FIGURE,
            pluralStringResource(R.plurals.home_figure_plays, plays),
            Modifier.weight(1f),
        )
        FigureRule()
        Figure(
            statistics?.listenedMs?.let { listenedHours(words(), it) } ?: NO_FIGURE,
            stringResource(R.string.home_figure_listened),
            Modifier.weight(1f),
        )
        FigureRule()
        Figure(
            songs.toString(),
            pluralStringResource(R.plurals.home_figure_songs, songs),
            Modifier.weight(1f),
        )
    }
}

@Composable
private fun FigureRule() {
    VerticalDivider(
        Modifier.padding(horizontal = 16.dp).fillMaxHeight(FIGURE_RULE_HEIGHT),
        color = MaterialTheme.colorScheme.outlineVariant,
    )
}

@Composable
private fun Figure(value: String, label: String, modifier: Modifier = Modifier) {
    Column(modifier) {
        Text(
            value,
            style = MaterialTheme.typography.headlineSmall,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
        Text(label, style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis)
    }
}

private val FIGURES_INSET = 8.dp

private const val NO_FIGURE = "–"
private const val FIGURE_RULE_HEIGHT = 0.7f
private const val MIX_TITLE_LINES = 2
private const val MIX_DETAIL_LINES = 2
private const val REASON_BADGE_ALPHA = 0.88f
private const val CAPTION_GONE_BELOW = 0.3f
private const val CAPTION_WHOLE_FROM = 0.5f
