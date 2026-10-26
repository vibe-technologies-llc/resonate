package org.resonate.android.ui.home

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.TrendingUp
import androidx.compose.material.icons.rounded.Add
import androidx.compose.material.icons.rounded.AutoAwesome
import androidx.compose.material.icons.rounded.Favorite
import androidx.compose.material.icons.rounded.HighQuality
import androidx.compose.material.icons.rounded.Insights
import androidx.compose.material.icons.rounded.NewReleases
import androidx.compose.material.icons.rounded.Shuffle
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialShapes
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedCard
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.carousel.HorizontalMultiBrowseCarousel
import androidx.compose.material3.carousel.rememberCarouselState
import androidx.compose.material3.toShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.RectangleShape
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import kotlin.math.roundToInt
import androidx.lifecycle.compose.LifecycleResumeEffect
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import java.time.LocalTime
import org.resonate.android.nativecore.AlbumHit
import org.resonate.android.nativecore.LibraryStatistics
import org.resonate.android.ui.LocalResonate
import org.resonate.android.ui.Mix
import org.resonate.android.ui.components.CollectionRow
import org.resonate.android.ui.components.IconTile
import org.resonate.android.ui.components.LocalSheets
import org.resonate.android.ui.components.Page
import org.resonate.android.ui.components.Picture
import org.resonate.android.ui.components.PlaylistCover
import org.resonate.android.ui.components.SectionHeader
import org.resonate.android.ui.components.SettingsAction
import org.resonate.android.ui.components.Sizes
import org.resonate.android.ui.components.TitleSize
import org.resonate.android.ui.components.rememberArtwork
import org.resonate.android.ui.counted
import org.resonate.android.ui.library.ArtistCard
import org.resonate.android.ui.library.LARGE_TEXT
import org.resonate.android.ui.library.Loading
import org.resonate.android.ui.library.PlaylistCard
import org.resonate.android.ui.library.Shelf
import org.resonate.android.ui.library.Shelves
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

@Composable
fun HomeScreen() {
    val model = resonateViewModel { HomeViewModel(it) }
    val ready by model.ready.collectAsStateWithLifecycle()
    val songs by model.songs.collectAsStateWithLifecycle()
    var hour by remember { mutableIntStateOf(LocalTime.now().hour) }
    val title = greeting(hour)
    LifecycleResumeEffect(model) {
        hour = LocalTime.now().hour
        model.refresh()
        onPauseOrDispose {}
    }
    Page(title, size = TitleSize.LARGE, back = false, actions = { SettingsAction() }) { padding ->
        when {
            !ready -> Loading(Modifier.padding(padding))
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
    LazyColumn(contentPadding = padding, modifier = Modifier.fillMaxSize().testTag("home")) {
        item("shortcuts") {
            val favouritesShortcut: @Composable (Modifier) -> Unit = { modifier ->
                Shortcut(
                    icon = Icons.Rounded.Favorite,
                    title = "Favourites",
                    detail = counted(favourites.size, "song"),
                    container = MaterialTheme.colorScheme.primaryContainer,
                    content = MaterialTheme.colorScheme.onPrimaryContainer,
                    badge = MaterialTheme.colorScheme.primary,
                    onBadge = MaterialTheme.colorScheme.onPrimary,
                    onClick = { navigator.open(Screen.Favourites) },
                    modifier = modifier.testTag("home-favourites"),
                )
            }
            val shuffleShortcut: @Composable (Modifier) -> Unit = { modifier ->
                Shortcut(
                    icon = Icons.Rounded.Shuffle,
                    title = "Shuffle all",
                    detail = counted(songs.size, "song"),
                    container = MaterialTheme.colorScheme.tertiaryContainer,
                    content = MaterialTheme.colorScheme.onTertiaryContainer,
                    badge = MaterialTheme.colorScheme.tertiary,
                    onBadge = MaterialTheme.colorScheme.onTertiary,
                    onClick = { resonate.playShuffled(songs) },
                    modifier = modifier.testTag("home-shuffle"),
                )
            }
            if (LocalDensity.current.fontScale > LARGE_TEXT) {
                Column(
                    Modifier.fillMaxWidth().padding(horizontal = Sizes.gutter, vertical = 8.dp),
                    verticalArrangement = Arrangement.spacedBy(12.dp),
                ) {
                    favouritesShortcut(Modifier.fillMaxWidth())
                    shuffleShortcut(Modifier.fillMaxWidth())
                }
            } else {
                Row(
                    Modifier.fillMaxWidth().padding(horizontal = Sizes.gutter, vertical = 8.dp),
                    horizontalArrangement = Arrangement.spacedBy(12.dp),
                ) {
                    favouritesShortcut(Modifier.weight(1f))
                    shuffleShortcut(Modifier.weight(1f))
                }
            }
        }
        mixes.valueOrNull()?.takeIf { it.isNotEmpty() }?.let { shown ->
            item("mixes-title") { SectionHeader("Made for you") }
            item("mixes") {
                Shelf(shown, key = { it.mix.name }) { shelf ->
                    MixCard(shelf, onClick = { navigator.open(Screen.Mix(shelf.mix)) })
                }
            }
        }
        albums.valueOrNull()?.takeIf { it.isNotEmpty() }?.let { shown ->
            item("recent-title") { SectionHeader("Recently added") }
            item("recent") { RecentAlbums(shown) { navigator.open(it.screen) } }
        }
        item("playlists-title") { SectionHeader("Your playlists") }
        if (playlists.isEmpty()) {
            item("playlists-none") {
                CollectionRow(
                    title = "New playlist",
                    supporting = "A mood, a moment, a journey. Make it a playlist.",
                    onClick = { sheets.name("New playlist") { model.createPlaylist(it) } },
                    leading = { IconTile(Icons.Rounded.Add) },
                    modifier = Modifier.testTag("home-new-playlist"),
                )
            }
        } else item("playlists") {
            Shelf(playlists + listOf(null), key = { it?.id?.toString() ?: "new" }) { playlist ->
                if (playlist == null) {
                    NewPlaylistCard { sheets.name("New playlist") { model.createPlaylist(it) } }
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
            item("artists-title") { SectionHeader("Artists") }
            item("artists") {
                Shelf(shown, key = { "artist:${it.name}" }) { artist ->
                    ArtistCard(
                        artist.shownName,
                        artist.artworkTrack,
                        counted(artist.songs, "song"),
                        onClick = { navigator.open(artist.screen) },
                    )
                }
            }
        }
        item("listening-title") { SectionHeader("Your listening") }
        item("listening") {
            ListeningCard(statistics, songs.size) { navigator.open(Screen.Statistics) }
        }
    }
}

@Composable
private fun Shortcut(
    icon: ImageVector,
    title: String,
    detail: String,
    container: Color,
    content: Color,
    badge: Color,
    onBadge: Color,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Surface(
        onClick = onClick,
        color = container,
        contentColor = content,
        shape = MaterialTheme.shapes.extraLarge,
        modifier = modifier.heightIn(min = 80.dp),
    ) {
        Row(
            Modifier.padding(horizontal = 14.dp, vertical = 14.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Surface(
                color = badge,
                contentColor = onBadge,
                shape = MaterialShapes.Cookie9Sided.toShape(),
                modifier = Modifier.size(44.dp),
            ) {
                Box(contentAlignment = Alignment.Center) { Icon(icon, contentDescription = null) }
            }
            Spacer(Modifier.width(12.dp))
            Column(Modifier.weight(1f)) {
                Text(title, style = MaterialTheme.typography.titleMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(detail, style = MaterialTheme.typography.bodySmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
        }
    }
}

private val Mix.icon: ImageVector
    get() =
        when (this) {
            Mix.NEVER_HEARD -> Icons.Rounded.AutoAwesome
            Mix.MOST_PLAYED -> Icons.AutoMirrored.Rounded.TrendingUp
            Mix.RECENTLY_ADDED -> Icons.Rounded.NewReleases
            Mix.HIGH_RESOLUTION -> Icons.Rounded.HighQuality
        }

private data class MixColours(val container: Color, val content: Color, val badge: Color, val onBadge: Color)

@Composable
private fun Mix.colours(): MixColours {
    val scheme = MaterialTheme.colorScheme
    return when (this) {
        Mix.NEVER_HEARD -> MixColours(scheme.tertiaryContainer, scheme.onTertiaryContainer, scheme.tertiary, scheme.onTertiary)
        Mix.MOST_PLAYED -> MixColours(scheme.primaryContainer, scheme.onPrimaryContainer, scheme.primary, scheme.onPrimary)
        Mix.RECENTLY_ADDED -> MixColours(scheme.secondaryContainer, scheme.onSecondaryContainer, scheme.secondary, scheme.onSecondary)
        Mix.HIGH_RESOLUTION -> MixColours(scheme.surfaceContainerHighest, scheme.onSurface, scheme.inverseSurface, scheme.inverseOnSurface)
    }
}

@Composable
private fun MixCard(shelf: MixShelf, onClick: () -> Unit) {
    val colours = shelf.mix.colours()
    Surface(
        onClick = onClick,
        color = colours.container,
        contentColor = colours.content,
        shape = MaterialTheme.shapes.extraLarge,
        modifier = Modifier.width(MIX_WIDTH).testTag("mix-${shelf.mix.name.lowercase()}"),
    ) {
        Column(Modifier.padding(12.dp)) {
            Box {
                PlaylistCover(shelf.covers, MIX_WIDTH - 24.dp, shape = MaterialTheme.shapes.large)
                Surface(
                    color = colours.badge,
                    contentColor = colours.onBadge,
                    shape = MaterialShapes.Cookie9Sided.toShape(),
                    modifier = Modifier.align(Alignment.BottomEnd).padding(8.dp).size(40.dp),
                ) {
                    Box(contentAlignment = Alignment.Center) {
                        Icon(shelf.mix.icon, contentDescription = null, modifier = Modifier.size(22.dp))
                    }
                }
            }
            Spacer(Modifier.height(12.dp))
            Text(shelf.mix.title, style = MaterialTheme.typography.titleMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(
                shelf.mix.subtitle,
                style = MaterialTheme.typography.bodySmall,
                maxLines = 2,
                minLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
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
        val shown = if (info.maxSize > info.minSize) ((info.size - info.minSize) / (info.maxSize - info.minSize)).coerceIn(0f, 1f) else 1f
        BoxWithConstraints(
            Modifier.fillMaxSize()
                .maskClip(MaterialTheme.shapes.extraLarge)
                .semantics(mergeDescendants = true) {
                    role = Role.Button
                    contentDescription = "${album.shownTitle}, ${album.artist}"
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
                        .graphicsLayer { alpha = shown * shown },
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
            "New playlist",
            style = MaterialTheme.typography.titleSmall,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.padding(horizontal = 4.dp),
        )
    }
}

@Composable
private fun ListeningCard(statistics: LibraryStatistics?, songs: Int, onClick: () -> Unit) {
    OutlinedCard(
        onClick = onClick,
        shape = MaterialTheme.shapes.extraLarge,
        modifier = Modifier.fillMaxWidth().padding(horizontal = Sizes.gutter, vertical = 4.dp).testTag("home-statistics"),
    ) {
        Column(Modifier.padding(20.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Icon(Icons.Rounded.Insights, contentDescription = null, tint = MaterialTheme.colorScheme.primary)
                Spacer(Modifier.width(12.dp))
                Text("Statistics", style = MaterialTheme.typography.titleMedium, modifier = Modifier.weight(1f))
            }
            Spacer(Modifier.height(16.dp))
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                Figure(statistics?.plays?.toString() ?: "–", "plays", Modifier.weight(1f))
                Figure(statistics?.listenedMs?.let(::listenedHours) ?: "–", "listened", Modifier.weight(1f))
                Figure((statistics?.songs?.toLong() ?: songs.toLong()).toString(), "songs", Modifier.weight(1f))
            }
        }
    }
}

@Composable
private fun Figure(value: String, label: String, modifier: Modifier = Modifier) {
    Column(modifier) {
        Text(value, style = MaterialTheme.typography.headlineSmall, color = MaterialTheme.colorScheme.primary, maxLines = 1, overflow = TextOverflow.Ellipsis)
        Text(label, style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis)
    }
}

private val MIX_WIDTH = 176.dp
