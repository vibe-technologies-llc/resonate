package org.resonate.android.ui.home

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.snapping.SnapPosition
import androidx.compose.foundation.gestures.snapping.rememberSnapFlingBehavior
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Favorite
import androidx.compose.material.icons.rounded.PlayArrow
import androidx.compose.material3.FilledIconButton
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.adaptive.currentWindowAdaptiveInfo
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.RectangleShape
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.hideFromAccessibility
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.min
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.window.core.layout.WindowSizeClass
import java.time.LocalDate
import org.resonate.android.LibraryFeatures
import org.resonate.android.R
import org.resonate.android.nativecore.AlbumHit
import org.resonate.android.nativecore.SuggestedList
import org.resonate.android.ui.components.PlaylistCover
import org.resonate.android.ui.components.SectionHeader
import org.resonate.android.ui.components.SongArtwork
import org.resonate.android.ui.components.Sizes
import org.resonate.android.ui.components.rememberLargeText
import org.resonate.android.ui.components.rememberTitleMaxLines
import org.resonate.android.ui.icon
import org.resonate.android.ui.library.ArtistCard
import org.resonate.android.ui.library.CoverCard
import org.resonate.android.ui.library.LibraryOpening
import org.resonate.android.ui.library.Shelf
import org.resonate.android.ui.library.Shelves
import org.resonate.android.ui.library.byline
import org.resonate.android.ui.library.rememberRowHeldAtItsHead
import org.resonate.android.ui.library.screen
import org.resonate.android.ui.library.showLibrary
import org.resonate.android.ui.library.shownTitle
import org.resonate.android.ui.navigation.Navigator
import org.resonate.android.ui.navigation.Screen
import org.resonate.android.ui.screen
import org.resonate.android.words

class HomeSections(
    val tiles: List<JumpBackTile>,
    val columns: Int,
    val mixes: List<SuggestedList>,
    val recent: List<AlbumHit>,
    val artists: List<PlayedArtist>,
    val heard: ListeningGlance.Heard?,
    val glanceDay: LocalDate?,
    val wide: Boolean,
)

@Composable
fun rememberCatalogSections(model: HomeViewModel): HomeSections {
    val played by model.played.collectAsStateWithLifecycle()
    val favourite by model.anyFavourite.collectAsStateWithLifecycle()
    val mixes by model.mixes.collectAsStateWithLifecycle()
    val recent by model.recent.collectAsStateWithLifecycle()
    val artists by model.artists.collectAsStateWithLifecycle()
    val heard by model.heard.collectAsStateWithLifecycle()
    val day by model.feed.day.collectAsStateWithLifecycle()
    val wide =
        currentWindowAdaptiveInfo()
            .windowSizeClass
            .isWidthAtLeastBreakpoint(WindowSizeClass.WIDTH_DP_MEDIUM_LOWER_BOUND)
    val columns = JumpBackGrid.columns(wide, rememberLargeText())
    return HomeSections(
        tiles = JumpBackIn.shown(played, favourite, columns),
        columns = columns,
        mixes = mixes,
        recent = recent,
        artists = artists,
        heard = heard,
        glanceDay = day,
        wide = wide,
    )
}

fun LazyListScope.catalogSections(
    sections: HomeSections,
    model: HomeViewModel,
    navigator: Navigator,
    features: LibraryFeatures,
    seeAll: String,
) {
    if (sections.tiles.isEmpty()) {
        recentlyAdded(sections.recent, model, navigator, features, seeAll)
        madeForYou(sections.mixes, model, navigator, seeAll)
    } else {
        item("jump-back-title") { SectionHeader(stringResource(R.string.home_jump_back_in)) }
        item("jump-back") { JumpBackGridOf(sections.tiles, sections.columns) { navigator.open(it.screen) } }
        madeForYou(sections.mixes, model, navigator, seeAll)
        recentlyAdded(sections.recent, model, navigator, features, seeAll)
    }
    if (sections.artists.isNotEmpty()) {
        item("artists-title") {
            SectionHeader(
                stringResource(R.string.home_artists),
                action = seeAll,
                onAction = { navigator.showLibrary(features, LibraryOpening.mostPlayedArtists) },
            )
        }
        item("artists") {
            Shelf(sections.artists, key = { "artist:${it.name}" }, Modifier.testTag("home-artists")) { artist ->
                ArtistCard(
                    artist.name.ifBlank { stringResource(R.string.unknown_artist) },
                    artist.cover,
                    pluralStringResource(R.plurals.home_artist_plays, artist.plays.toInt(), artist.plays.toInt()),
                    onClick = { navigator.open(Screen.Artist(artist.name, artist.cover)) },
                )
            }
        }
    }
}

private fun LazyListScope.madeForYou(
    mixes: List<SuggestedList>,
    model: HomeViewModel,
    navigator: Navigator,
    seeAll: String,
) {
    if (mixes.isEmpty()) return
    item("mixes-title") {
        SectionHeader(
            stringResource(R.string.library_made_for_you),
            action = seeAll,
            onAction = { navigator.open(Screen.Suggestions) },
        )
    }
    item("mixes") {
        Shelf(mixes, key = { it.id }, Modifier.testTag("home-mixes")) { list ->
            MixCard(list, onOpen = { navigator.open(list.screen) }, onPlay = { model.playMix(list) })
        }
    }
}

private fun LazyListScope.recentlyAdded(
    albums: List<AlbumHit>,
    model: HomeViewModel,
    navigator: Navigator,
    features: LibraryFeatures,
    seeAll: String,
) {
    if (albums.isEmpty()) return
    item("recent-title") {
        SectionHeader(
            stringResource(R.string.home_recently_added),
            action = seeAll,
            onAction = { navigator.showLibrary(features, LibraryOpening.recentlyAdded) },
        )
    }
    item("recent") { RecentAlbums(albums, onOpen = { navigator.open(it.screen) }, onPlay = model::playAlbum) }
}

@Composable
private fun JumpBackGridOf(tiles: List<JumpBackTile>, columns: Int, onOpen: (JumpBackTile) -> Unit) {
    Column(
        Modifier.fillMaxWidth().padding(horizontal = Sizes.gutter, vertical = 4.dp).testTag("jump-back-in"),
        verticalArrangement = Arrangement.spacedBy(TILE_GAP),
    ) {
        tiles.chunked(columns).forEach { row ->
            Row(Modifier.fillMaxWidth().height(IntrinsicSize.Min), horizontalArrangement = Arrangement.spacedBy(TILE_GAP)) {
                row.forEach { tile ->
                    JumpBackTileView(tile, Modifier.weight(1f).fillMaxHeight()) { onOpen(tile) }
                }
                repeat(columns - row.size) { Spacer(Modifier.weight(1f)) }
            }
        }
    }
}

@Composable
private fun JumpBackTileView(tile: JumpBackTile, modifier: Modifier, onClick: () -> Unit) {
    val scheme = MaterialTheme.colorScheme
    val words = words()
    val spoken = tile.spoken(words)
    val edge = TILE_COVER * LocalDensity.current.fontScale.coerceAtMost(TILE_COVER_GROWS_TO)
    Row(
        modifier
            .heightIn(min = edge)
            .clip(MaterialTheme.shapes.medium)
            .background(scheme.surfaceContainerHigh)
            .clickable(role = Role.Button, onClick = onClick)
            .semantics { contentDescription = spoken }
            .testTag("jump-back-tile"),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(Modifier.size(edge).clearAndSetSemantics {}) {
            when (tile) {
                is JumpBackTile.Album -> SongArtwork(tile.cover, edge, shape = RectangleShape)
                JumpBackTile.Favourites ->
                    Box(Modifier.size(edge).background(scheme.surfaceContainerHighest), contentAlignment = Alignment.Center) {
                        Icon(Icons.Rounded.Favorite, contentDescription = null, tint = scheme.primary)
                    }
            }
        }
        Spacer(Modifier.width(10.dp))
        Text(
            tile.title(words),
            style = MaterialTheme.typography.titleSmall,
            maxLines = maxOf(TILE_TITLE_LINES, rememberTitleMaxLines()),
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.weight(1f).padding(end = 10.dp, top = 6.dp, bottom = 6.dp).clearAndSetSemantics {},
        )
    }
}

@Composable
private fun MixCard(list: SuggestedList, onOpen: () -> Unit, onPlay: () -> Unit) {
    val scheme = MaterialTheme.colorScheme
    val lines = rememberTitleMaxLines()
    CoverCard(
        title = list.name,
        detail = list.detail,
        onClick = onOpen,
        modifier = Modifier.width(Shelves.playlistWidth).testTag("mix-${list.reason.name.lowercase()}"),
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
                    Icon(list.reason.icon, contentDescription = null, modifier = Modifier.size(16.dp))
                }
            }
            PlayOnCover(list.name, onPlay, Modifier.align(Alignment.BottomEnd).padding(6.dp))
        }
    }
}

@Composable
private fun RecentAlbums(albums: List<AlbumHit>, onOpen: (AlbumHit) -> Unit, onPlay: (AlbumHit) -> Unit) {
    val list = rememberRowHeldAtItsHead(albums.firstOrNull()?.key)
    val edge = min(RECENT_COVER * LocalDensity.current.fontScale, RECENT_COVER_WIDEST)
    LazyRow(
        state = list,
        flingBehavior = rememberSnapFlingBehavior(list, SnapPosition.Start),
        contentPadding = PaddingValues(horizontal = Sizes.gutter),
        horizontalArrangement = Arrangement.spacedBy(14.dp),
        modifier = Modifier.fillMaxWidth().padding(vertical = 4.dp).testTag("recent-albums"),
    ) {
        items(albums.size, key = { albums[it].key }) { index ->
            RecentAlbumCard(albums[index], edge, onOpen, onPlay)
        }
    }
}

@Composable
private fun RecentAlbumCard(album: AlbumHit, edge: Dp, onOpen: (AlbumHit) -> Unit, onPlay: (AlbumHit) -> Unit) {
    val lines = rememberTitleMaxLines()
    val title = album.shownTitle
    Column(Modifier.width(edge).testTag("recent-album")) {
        Box {
            Box(
                Modifier.clip(MaterialTheme.shapes.extraLarge)
                    .semantics { hideFromAccessibility() }
                    .clickable { onOpen(album) }
            ) {
                SongArtwork(album.artworkTrack, edge, shape = MaterialTheme.shapes.extraLarge)
            }
            PlayOnCover(title, { onPlay(album) }, Modifier.align(Alignment.BottomEnd).padding(10.dp))
        }
        Spacer(Modifier.height(8.dp))
        Column(
            Modifier.fillMaxWidth()
                .clip(MaterialTheme.shapes.small)
                .clickable(role = Role.Button) { onOpen(album) }
                .padding(horizontal = 4.dp, vertical = 2.dp)
        ) {
            Text(title, style = MaterialTheme.typography.titleSmall, maxLines = lines, overflow = TextOverflow.Ellipsis)
            Text(
                album.byline,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = lines,
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}

@Composable
private fun PlayOnCover(named: String, onClick: () -> Unit, modifier: Modifier = Modifier) {
    FilledIconButton(onClick = onClick, modifier = modifier.size(PLAY_BUTTON).testTag("play-on-cover")) {
        Icon(Icons.Rounded.PlayArrow, contentDescription = stringResource(R.string.home_play_named, named))
    }
}

private val TILE_GAP = 10.dp
private val TILE_COVER = 56.dp
private val RECENT_COVER = 200.dp
private val RECENT_COVER_WIDEST = 260.dp
private val PLAY_BUTTON = 44.dp

private const val TILE_COVER_GROWS_TO = 1.3f
private const val TILE_TITLE_LINES = 2
private const val MIX_TITLE_LINES = 2
private const val MIX_DETAIL_LINES = 2
private const val REASON_BADGE_ALPHA = 0.88f
