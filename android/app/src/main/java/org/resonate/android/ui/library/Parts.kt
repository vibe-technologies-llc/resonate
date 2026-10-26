package org.resonate.android.ui.library

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.CreateNewFolder
import androidx.compose.material.icons.rounded.Download
import androidx.compose.material.icons.rounded.ErrorOutline
import androidx.compose.material.icons.rounded.LibraryMusic
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialShapes
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.toShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import org.resonate.android.nativecore.AlbumHit
import org.resonate.android.nativecore.ArtistHit
import org.resonate.android.nativecore.MusicPlaylist
import org.resonate.android.ui.LocalResonate
import org.resonate.android.ui.components.ArtistPortrait
import org.resonate.android.ui.components.EmptyState
import org.resonate.android.ui.components.PlayShuffleButtons
import org.resonate.android.ui.components.PlaylistCover
import org.resonate.android.ui.components.Sizes
import org.resonate.android.ui.components.SongArtwork
import org.resonate.android.ui.counted
import org.resonate.android.ui.navigation.Screen

object Shelves {
    val albumWidth = 152.dp
    val playlistWidth = 152.dp
    val artistWidth = 112.dp
    val spacing = 12.dp
    val pageWidest = 840.dp
}

val AlbumHit.screen: Screen.Album
    get() = Screen.Album(key, title, artist, artworkTrack)

val ArtistHit.screen: Screen.Artist
    get() = Screen.Artist(name, artworkTrack)

val ArtistHit.shownName: String
    get() = name.ifBlank { "Unknown artist" }

val AlbumHit.shownTitle: String
    get() = title.ifBlank { "Unknown album" }

val AlbumHit.byline: String
    get() =
        listOfNotNull(
                if (variousArtists) "Various artists" else artist.ifBlank { "Unknown artist" },
                year?.toString(),
            )
            .joinToString(" · ")

val ArtistHit.summary: String
    get() =
        if (albums > 0u) "${counted(albums, "album")} · ${counted(songs, "song")}"
        else counted(songs, "song")

val MusicPlaylist.screen: Screen.Playlist
    get() = Screen.Playlist(id, name)

@Composable
fun SquareCover(song: ULong?, modifier: Modifier = Modifier, shape: Shape = MaterialTheme.shapes.large) {
    BoxWithConstraints(modifier.fillMaxWidth().aspectRatio(1f)) {
        SongArtwork(song, maxWidth, shape = shape)
    }
}

@Composable
fun AlbumCard(album: AlbumHit, onClick: () -> Unit, modifier: Modifier = Modifier) {
    CoverCard(
        title = album.shownTitle,
        detail = album.byline,
        onClick = onClick,
        modifier = modifier,
    ) {
        SquareCover(album.artworkTrack)
    }
}

@Composable
fun PlaylistCard(playlist: MusicPlaylist, onClick: () -> Unit, modifier: Modifier = Modifier) {
    CoverCard(
        title = playlist.name,
        detail = counted(playlist.songs, "song"),
        onClick = onClick,
        modifier = modifier,
    ) {
        BoxWithConstraints(Modifier.fillMaxWidth().aspectRatio(1f)) {
            PlaylistCover(playlist.covers, maxWidth, shape = MaterialTheme.shapes.large)
        }
    }
}

@Composable
fun CoverCard(
    title: String,
    detail: String?,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    cover: @Composable () -> Unit,
) {
    Column(
        modifier
            .clip(MaterialTheme.shapes.large)
            .clickable(onClick = onClick, role = Role.Button)
            .semantics(mergeDescendants = true) {}
    ) {
        cover()
        Spacer(Modifier.height(8.dp))
        Text(
            title,
            style = MaterialTheme.typography.titleSmall,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.padding(horizontal = 4.dp),
        )
        if (detail != null) {
            Text(
                detail,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.padding(horizontal = 4.dp).padding(bottom = 4.dp),
            )
        }
    }
}

@Composable
fun ArtistCard(
    name: String,
    song: ULong?,
    detail: String?,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Column(
        modifier
            .width(Shelves.artistWidth)
            .clip(MaterialTheme.shapes.large)
            .clickable(onClick = onClick, role = Role.Button)
            .semantics(mergeDescendants = true) {}
            .padding(vertical = 4.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        ArtistPortrait(song, Shelves.artistWidth - 8.dp)
        Spacer(Modifier.height(8.dp))
        Text(
            name,
            style = MaterialTheme.typography.titleSmall,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            textAlign = TextAlign.Center,
        )
        if (detail != null) {
            Text(
                detail,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                textAlign = TextAlign.Center,
            )
        }
    }
}

@Composable
fun <T> Shelf(
    items: List<T>,
    key: (T) -> Any,
    modifier: Modifier = Modifier,
    content: @Composable (T) -> Unit,
) {
    LazyRow(
        modifier = modifier.fillMaxWidth(),
        contentPadding = PaddingValues(horizontal = Sizes.gutter),
        horizontalArrangement = Arrangement.spacedBy(Shelves.spacing),
    ) {
        items(items.size, key = { key(items[it]) }) { content(items[it]) }
    }
}

@Composable
fun rememberPastHeader(list: LazyListState): Boolean {
    val past by remember(list) { derivedStateOf { list.firstVisibleItemIndex > 0 } }
    return past
}

@Composable
fun CollectionHeader(
    title: String,
    summary: String,
    onPlay: () -> Unit,
    onShuffle: () -> Unit,
    modifier: Modifier = Modifier,
    playable: Boolean = true,
    overline: String? = null,
    link: (@Composable () -> Unit)? = null,
    extra: (@Composable () -> Unit)? = null,
    coverShape: Shape = MaterialTheme.shapes.extraLarge,
    cover: @Composable (Dp) -> Unit,
) {
    BoxWithConstraints(modifier.fillMaxWidth()) {
        val wide = maxWidth >= 600.dp
        val side = if (wide) minOf(maxWidth * 0.32f, Sizes.headerCover) else minOf(maxWidth - 96.dp, Sizes.headerCover + 24.dp)
        val details: @Composable (Alignment.Horizontal, TextAlign) -> Unit = { alignment, align ->
            Column(horizontalAlignment = alignment, modifier = Modifier.fillMaxWidth()) {
                if (overline != null) {
                    Text(
                        overline,
                        style = MaterialTheme.typography.labelLarge,
                        color = MaterialTheme.colorScheme.primary,
                        textAlign = align,
                    )
                    Spacer(Modifier.height(4.dp))
                }
                Text(
                    title,
                    style = MaterialTheme.typography.headlineMedium,
                    textAlign = align,
                    maxLines = 3,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.semantics { heading() },
                )
                if (link != null) {
                    Spacer(Modifier.height(8.dp))
                    link()
                }
                Spacer(Modifier.height(8.dp))
                Text(
                    summary,
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    textAlign = align,
                )
                if (extra != null) {
                    Spacer(Modifier.height(12.dp))
                    extra()
                }
            }
        }
        if (wide) {
            Column(Modifier.padding(vertical = 8.dp)) {
                Row(
                    Modifier.fillMaxWidth().padding(horizontal = Sizes.gutter),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(24.dp),
                ) {
                    Box(Modifier.shadow(8.dp, coverShape)) { cover(side) }
                    Box(Modifier.weight(1f)) { details(Alignment.Start, TextAlign.Start) }
                }
                if (playable) {
                    Spacer(Modifier.height(20.dp))
                    PlayShuffleButtons(onPlay, onShuffle, modifier = Modifier.widthIn(max = 520.dp))
                }
            }
        } else {
            Column(
                Modifier.fillMaxWidth().padding(top = 8.dp, bottom = 8.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Box(Modifier.shadow(12.dp, coverShape)) { cover(side) }
                Spacer(Modifier.height(20.dp))
                Box(Modifier.padding(horizontal = 24.dp)) {
                    details(Alignment.CenterHorizontally, TextAlign.Center)
                }
                if (playable) {
                    Spacer(Modifier.height(20.dp))
                    PlayShuffleButtons(onPlay, onShuffle)
                }
            }
        }
    }
}

@Composable
fun ArtistLink(name: String, song: ULong?, onClick: (() -> Unit)?) {
    Surface(
        onClick = onClick ?: {},
        enabled = onClick != null,
        shape = CircleShape,
        color = MaterialTheme.colorScheme.surfaceContainerHigh,
    ) {
        Row(
            Modifier.padding(start = 4.dp, end = 16.dp, top = 4.dp, bottom = 4.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            ArtistPortrait(song, 32.dp)
            Spacer(Modifier.width(8.dp))
            Text(
                name,
                style = MaterialTheme.typography.titleSmall,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}

@Composable
fun ShapeCover(side: Dp, shape: Shape) {
    Surface(
        color = MaterialTheme.colorScheme.primaryContainer,
        shape = MaterialTheme.shapes.extraLarge,
        modifier = Modifier.size(side),
    ) {
        Box(contentAlignment = Alignment.Center) {
            Surface(color = MaterialTheme.colorScheme.primary, shape = shape, modifier = Modifier.size(side * 0.5f)) {}
        }
    }
}

@Composable
fun Loading(modifier: Modifier = Modifier) {
    Box(modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
        LoadingIndicator(Modifier.testTag("loading"))
    }
}

@Composable
fun Unreadable(onRetry: () -> Unit, modifier: Modifier = Modifier) {
    EmptyState(
        Icons.Rounded.ErrorOutline,
        "This could not be loaded",
        modifier = modifier,
        detail = "Check the notice below, then try again.",
        action = "Try again",
        onAction = onRetry,
    )
}

@Composable
fun <T> LoadedContent(
    loaded: Loaded<T>,
    onRetry: () -> Unit,
    modifier: Modifier = Modifier,
    content: @Composable (T) -> Unit,
) {
    when (loaded) {
        Loaded.Loading -> Loading(modifier)
        Loaded.Failed -> Unreadable(onRetry, modifier)
        is Loaded.Ready -> content(loaded.value)
    }
}

@Composable
fun Welcome(modifier: Modifier = Modifier) {
    val host = LocalResonate.current.host
    BoxWithConstraints(modifier.fillMaxSize()) {
        Column(
            Modifier.fillMaxWidth().verticalScroll(rememberScrollState()).heightIn(min = maxHeight).padding(32.dp),
            verticalArrangement = Arrangement.Center,
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Surface(
                color = MaterialTheme.colorScheme.primaryContainer,
                contentColor = MaterialTheme.colorScheme.onPrimaryContainer,
                shape = MaterialShapes.Cookie9Sided.toShape(),
                modifier = Modifier.size(112.dp),
            ) {
                Box(contentAlignment = Alignment.Center) {
                    Icon(Icons.Rounded.LibraryMusic, contentDescription = null, modifier = Modifier.size(48.dp))
                }
            }
            Spacer(Modifier.height(24.dp))
            Text(
                "Make room for your music",
                style = MaterialTheme.typography.headlineSmall,
                textAlign = TextAlign.Center,
                modifier = Modifier.widthIn(max = 420.dp),
            )
            Spacer(Modifier.height(8.dp))
            Text(
                "Add a music folder, or use your Downloads, to bring your albums, artists and favourites together.",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                textAlign = TextAlign.Center,
                modifier = Modifier.widthIn(max = 420.dp),
            )
            Spacer(Modifier.height(24.dp))
            Button(onClick = host::chooseFolder, modifier = Modifier.testTag("add-folder")) {
                Icon(Icons.Rounded.CreateNewFolder, null, Modifier.size(ButtonDefaults.IconSize))
                Spacer(Modifier.width(ButtonDefaults.IconSpacing))
                Text("Add music folder")
            }
            Spacer(Modifier.height(8.dp))
            OutlinedButton(onClick = host::useDownloads, modifier = Modifier.testTag("use-downloads")) {
                Icon(Icons.Rounded.Download, null, Modifier.size(ButtonDefaults.IconSize))
                Spacer(Modifier.width(ButtonDefaults.IconSpacing))
                Text("Use Downloads")
            }
        }
    }
}

@Composable
fun EmptyCollection(modifier: Modifier = Modifier, detail: String = "Add some songs to make this collection yours.") {
    Column(
        modifier.fillMaxWidth().padding(horizontal = 32.dp, vertical = 24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text("A little quiet here", style = MaterialTheme.typography.titleMedium, textAlign = TextAlign.Center)
        Spacer(Modifier.height(4.dp))
        Text(
            detail,
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            textAlign = TextAlign.Center,
            modifier = Modifier.widthIn(max = 420.dp),
        )
    }
}

fun LazyListScope.sectionTitle(key: String, title: String, action: String? = null, onAction: (() -> Unit)? = null) {
    item(key) { org.resonate.android.ui.components.SectionHeader(title, action = action, onAction = onAction) }
}
