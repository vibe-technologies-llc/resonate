package org.resonate.android

import android.net.Uri
import androidx.compose.animation.Crossfade
import androidx.compose.animation.core.Spring
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.spring
import androidx.compose.animation.core.tween
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.PlaylistPlay
import androidx.compose.material.icons.automirrored.filled.Undo
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Favorite
import androidx.compose.material.icons.filled.FavoriteBorder
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.PushPin
import androidx.compose.material.icons.filled.Search
import androidx.compose.material.icons.filled.Shuffle
import androidx.compose.material3.Button
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.IconToggleButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.resonate.android.nativecore.MusicPlaylist
import org.resonate.android.nativecore.Track

fun albumDestination(title: String, artist: String) =
    "Album?title=${Uri.encode(title)}&artist=${Uri.encode(artist)}"

fun artistDestination(name: String) = "Artist?name=${Uri.encode(name)}"

@Composable
fun LibraryScreen(
    library: LibraryRepository,
    navigate: (String) -> Unit,
    playlistsOnly: Boolean = false,
) {
    val tracks by library.allTracks.collectAsState()
    val playlists by library.features.playlists.collectAsState()
    var query by rememberSaveable { mutableStateOf("") }
    var category by rememberSaveable { mutableStateOf(if (playlistsOnly) "Playlists" else "All") }
    var creating by remember { mutableStateOf(false) }
    LaunchedEffect(Unit) { library.features.loadPlaylists() }
    val albums =
        tracks
            .groupBy { it.album to it.albumArtist.ifBlank { it.artist } }
            .entries
            .filter { query.isBlank() || "${it.key.first} ${it.key.second}".contains(query, true) }
    val artists =
        tracks
            .groupBy { it.artist }
            .entries
            .filter { query.isBlank() || it.key.contains(query, true) }
    val visiblePlaylists = playlists.filter { it.name.contains(query, true) }
    LazyColumn(
        Modifier.fillMaxSize(),
        contentPadding = PaddingValues(bottom = 24.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        item(key = "search") {
            OutlinedTextField(
                query,
                { query = it },
                placeholder = { Text("Search your library") },
                leadingIcon = { Icon(Icons.Default.Search, null) },
                singleLine = true,
                shape = RoundedCornerShape(20.dp),
                modifier =
                    Modifier.fillMaxWidth().padding(horizontal = 16.dp).semantics {
                        contentDescription = "Search your library"
                    },
            )
        }
        if (!playlistsOnly)
            item(key = "categories") {
                Row(
                    Modifier.horizontalScroll(rememberScrollState()).padding(horizontal = 16.dp),
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    listOf("All", "Playlists", "Albums", "Artists").forEach { label ->
                        FilterChip(category == label, { category = label }, label = { Text(label) })
                    }
                }
            }
        item(key = "favourites") {
            CollectionRow(
                "Favourites",
                "Pinned · ${songCount(tracks.count { it.favourite })}",
                pinned = true,
                artwork = { FavouritesArtwork(Modifier.size(64.dp)) },
            ) {
                navigate("Favourites")
            }
        }
        if (!playlistsOnly && category == "All" && query.isBlank())
            item(key = "all-songs") {
                CollectionRow(
                    "All songs",
                    songCount(tracks.size),
                    artwork = { MusicTile(Modifier.size(64.dp)) },
                ) {
                    navigate("Songs")
                }
            }
        if (category in listOf("All", "Playlists")) {
            item(key = "playlist-heading") {
                CollectionHeading(
                    "Playlists",
                    action = {
                        IconButton(
                            onClick = { library.features.editPlaylist { it.undoPlaylist() } }
                        ) {
                            Icon(Icons.AutoMirrored.Filled.Undo, "Undo last playlist edit")
                        }
                        IconButton(onClick = { creating = true }) {
                            Icon(Icons.Default.Add, "Create playlist")
                        }
                    },
                )
            }
            items(visiblePlaylists, key = { "playlist:${it.id}" }) { playlist ->
                CollectionRow(
                    playlist.name,
                    "Playlist · ${songCount(playlist.songs.toInt())}",
                    modifier = Modifier.animateItem(),
                    artwork = { PlaylistArtwork(playlist, library, Modifier.size(64.dp)) },
                ) {
                    navigate("Playlist?id=${playlist.id}")
                }
            }
            if (visiblePlaylists.isEmpty())
                item(key = "no-playlists") {
                    Text(
                        if (query.isBlank())
                            "Make a playlist for a mood, a moment, or your next journey."
                        else "No playlists match “$query”",
                        Modifier.padding(horizontal = 20.dp, vertical = 12.dp),
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    if (query.isBlank())
                        OutlinedButton(
                            onClick = { creating = true },
                            modifier = Modifier.padding(horizontal = 20.dp),
                        ) {
                            Icon(Icons.Default.Add, null)
                            Text("Create playlist", Modifier.padding(start = 8.dp))
                        }
                }
        }
        if (!playlistsOnly && category in listOf("All", "Albums")) {
            item(key = "album-heading") { CollectionHeading("Albums") }
            items(
                albums,
                key = { "album:${it.key.first.length}:${it.key.first}${it.key.second}" },
            ) { album ->
                AlbumRow(album.value, library, Modifier.animateItem()) {
                    navigate(albumDestination(album.key.first, album.key.second))
                }
            }
            if (albums.isEmpty())
                item(key = "no-albums") {
                    CollectionMessage(
                        if (tracks.isEmpty())
                            "Add a music folder in Settings to see your albums here."
                        else "No albums match “$query”"
                    )
                }
        }
        if (!playlistsOnly && category == "Artists") {
            item(key = "artist-heading") { CollectionHeading("Artists") }
            items(artists, key = { "artist:${it.key}" }) { artist ->
                CollectionRow(
                    artist.key.ifBlank { "Unknown artist" },
                    "Artist · ${songCount(artist.value.size)}",
                    artwork = {
                        Artwork(Modifier.size(64.dp), artist.value.first(), library, CircleShape)
                    },
                ) {
                    navigate(artistDestination(artist.key))
                }
            }
            if (artists.isEmpty())
                item(key = "no-artists") {
                    CollectionMessage(
                        if (tracks.isEmpty())
                            "Add a music folder in Settings to see your artists here."
                        else "No artists match “$query”"
                    )
                }
        }
    }
    if (creating) PlaylistNameDialog(library, dismiss = { creating = false })
}

@Composable
fun CollectionRow(
    title: String,
    subtitle: String,
    artwork: @Composable () -> Unit,
    modifier: Modifier = Modifier,
    pinned: Boolean = false,
    open: () -> Unit,
) {
    Surface(
        modifier = if (pinned) modifier.padding(horizontal = 12.dp) else modifier,
        shape = RoundedCornerShape(24.dp),
        color =
            if (pinned) MaterialTheme.colorScheme.secondaryContainer.copy(alpha = 0.38f)
            else Color.Transparent,
        border =
            if (pinned) BorderStroke(1.dp, MaterialTheme.colorScheme.primary.copy(alpha = 0.14f))
            else null,
    ) {
        Row(
            Modifier.fillMaxWidth()
                .clickable(onClick = open)
                .padding(horizontal = 20.dp, vertical = 10.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            artwork()
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Text(
                    title,
                    style = MaterialTheme.typography.titleMedium,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                )
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(6.dp),
                ) {
                    if (pinned)
                        Icon(
                            Icons.Default.PushPin,
                            null,
                            Modifier.size(14.dp),
                            tint = MaterialTheme.colorScheme.primary,
                        )
                    Text(
                        subtitle,
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 2,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
            }
        }
    }
}

@Composable
fun AlbumRow(
    rows: List<Track>,
    library: LibraryRepository,
    modifier: Modifier = Modifier,
    open: () -> Unit,
) {
    val first = rows.first()
    CollectionRow(
        first.album.ifBlank { "Unknown album" },
        "Album · ${first.albumArtist.ifBlank { first.artist }.ifBlank { "Unknown artist" }}",
        artwork = { Artwork(Modifier.size(64.dp), first, library) },
        modifier = modifier,
        open = open,
    )
}

@Composable
fun CollectionHeading(title: String, action: @Composable () -> Unit = {}) {
    Row(
        Modifier.fillMaxWidth().padding(start = 20.dp, end = 12.dp, top = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            title,
            Modifier.weight(1f),
            style = MaterialTheme.typography.titleLarge,
            fontWeight = FontWeight.SemiBold,
        )
        action()
    }
}

@Composable
fun CollectionMessage(message: String) {
    Text(message, Modifier.padding(20.dp), color = MaterialTheme.colorScheme.onSurfaceVariant)
}

@Composable
fun FavouritesArtwork(modifier: Modifier) {
    Surface(modifier, shape = RoundedCornerShape(12.dp)) {
        Box(
            Modifier.fillMaxSize()
                .background(
                    Brush.linearGradient(
                        listOf(
                            MaterialTheme.colorScheme.primary,
                            MaterialTheme.colorScheme.tertiaryContainer,
                        )
                    )
                ),
            contentAlignment = Alignment.Center,
        ) {
            Icon(
                Icons.Default.Favorite,
                null,
                Modifier.fillMaxSize(0.42f),
                tint = MaterialTheme.colorScheme.onPrimary,
            )
        }
    }
}

@Composable
private fun MusicTile(modifier: Modifier) {
    Surface(
        modifier,
        shape = RoundedCornerShape(12.dp),
        color = MaterialTheme.colorScheme.secondaryContainer,
    ) {
        Box(contentAlignment = Alignment.Center) {
            Icon(
                Icons.AutoMirrored.Filled.PlaylistPlay,
                null,
                Modifier.size(32.dp),
                tint = MaterialTheme.colorScheme.onSecondaryContainer,
            )
        }
    }
}

@Composable
fun PlaylistArtwork(playlist: MusicPlaylist, library: LibraryRepository, modifier: Modifier) {
    val generation by library.features.artworkGeneration.collectAsState()
    val rows by
        produceState<List<Track>>(emptyList(), playlist.id, playlist.revision, generation) {
            value =
                withContext(Dispatchers.IO) {
                    try {
                        library.native().playlistTracks(playlist.id, 0u, 4u).distinctBy {
                            it.album to it.albumArtist.ifBlank { it.artist }
                        }
                    } catch (cancelled: kotlinx.coroutines.CancellationException) {
                        throw cancelled
                    } catch (failure: Exception) {
                        emptyList()
                    }
                }
        }
    Surface(
        modifier,
        shape = RoundedCornerShape(12.dp),
        color = MaterialTheme.colorScheme.secondaryContainer,
    ) {
        when (rows.size) {
            0 ->
                Box(contentAlignment = Alignment.Center) {
                    Icon(
                        Icons.AutoMirrored.Filled.PlaylistPlay,
                        null,
                        Modifier.fillMaxSize(0.45f),
                        tint = MaterialTheme.colorScheme.onSecondaryContainer,
                    )
                }
            1 -> Artwork(Modifier.fillMaxSize(), rows.first(), library)
            else ->
                Column {
                    repeat(2) { y ->
                        Row(Modifier.weight(1f)) {
                            repeat(2) { x ->
                                Artwork(
                                    Modifier.weight(1f).aspectRatio(1f),
                                    rows[(y * 2 + x) % rows.size],
                                    library,
                                    RoundedCornerShape(0.dp),
                                )
                            }
                        }
                    }
                }
        }
    }
}

@Composable
fun FavouriteButton(track: Track?, library: LibraryRepository, current: Boolean = false) {
    val tracks by library.allTracks.collectAsState()
    val value = tracks.find { it.id == track?.id } ?: track
    val checked = value?.favourite == true
    val subject = if (current) "current song" else value?.title.orEmpty()
    val scale =
        animateFloatAsState(
            if (checked) 1.15f else 1f,
            spring(
                dampingRatio = Spring.DampingRatioMediumBouncy,
                stiffness = Spring.StiffnessMedium,
            ),
            label = "Favourite heart response",
        )
    IconToggleButton(
        checked = checked,
        onCheckedChange = { enabled -> value?.let { library.setFavourite(it.id, enabled) } },
        enabled = value != null,
        modifier =
            Modifier.semantics {
                contentDescription =
                    if (checked) "Remove $subject from favourites" else "Add $subject to favourites"
            },
    ) {
        Crossfade(checked, animationSpec = tween(160), label = "Favourite heart fill") { active ->
            Icon(
                if (active) Icons.Default.Favorite else Icons.Default.FavoriteBorder,
                null,
                Modifier.graphicsLayer {
                    scaleX = scale.value
                    scaleY = scale.value
                },
                tint =
                    if (active) MaterialTheme.colorScheme.primary
                    else MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

@Composable
fun CollectionHeader(
    title: String,
    subtitle: String,
    artwork: @Composable () -> Unit,
    play: () -> Unit,
    shuffle: () -> Unit,
    enabled: Boolean,
    detail: @Composable () -> Unit = {},
    track: Track? = null,
    library: LibraryRepository? = null,
    artworkShape: Shape = RoundedCornerShape(24.dp),
) {
    val tones by rememberArtworkTones(track, library)
    AmbientBackdrop(
        tones,
        Modifier.fillMaxWidth()
            .padding(horizontal = 12.dp, vertical = 8.dp)
            .clip(RoundedCornerShape(32.dp)),
        strength = 0.24f,
    ) {
        Column(
            Modifier.fillMaxWidth().padding(20.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            Box(
                Modifier.fillMaxWidth().padding(vertical = 8.dp),
                contentAlignment = Alignment.Center,
            ) {
                Box(Modifier.shadow(20.dp, artworkShape).clip(artworkShape)) { artwork() }
            }
            Text(
                title,
                style = MaterialTheme.typography.headlineLarge,
                fontWeight = FontWeight.Bold,
            )
            detail()
            Text(
                subtitle,
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                Button(onClick = play, enabled = enabled, modifier = Modifier.weight(1f)) {
                    Icon(Icons.Default.PlayArrow, null)
                    Text("Play", Modifier.padding(start = 8.dp))
                }
                OutlinedButton(
                    onClick = shuffle,
                    enabled = enabled,
                    modifier = Modifier.weight(1f),
                ) {
                    Icon(Icons.Default.Shuffle, null)
                    Text("Shuffle", Modifier.padding(start = 8.dp))
                }
            }
        }
    }
}

@Composable
fun CollectionTrackRow(
    track: Track,
    index: Int,
    library: LibraryRepository,
    playing: Boolean,
    play: () -> Unit,
    modifier: Modifier = Modifier,
    actions: @Composable () -> Unit = {},
) {
    Row(
        modifier
            .fillMaxWidth()
            .clickable(onClick = play)
            .semantics { contentDescription = "Play song ${index + 1}: ${track.title}" }
            .padding(start = 20.dp, end = 8.dp, top = 6.dp, bottom = 6.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            (index + 1).toString(),
            Modifier.padding(end = 12.dp),
            style = MaterialTheme.typography.labelMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Artwork(Modifier.size(44.dp), track, library)
        Column(Modifier.weight(1f).padding(horizontal = 12.dp)) {
            Text(
                track.title,
                style = MaterialTheme.typography.titleSmall,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                color =
                    if (playing) MaterialTheme.colorScheme.primary
                    else MaterialTheme.colorScheme.onSurface,
            )
            Text(
                track.artist.ifBlank { "Unknown artist" },
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
        actions()
    }
}

@Composable
fun FavouritesScreen(
    library: LibraryRepository,
    play: (List<Track>, Int, Boolean) -> Unit,
    browse: () -> Unit,
) {
    val tracks by library.allTracks.collectAsState()
    val snapshot by library.snapshot.collectAsState()
    var query by rememberSaveable { mutableStateOf("") }
    val rows = tracks.filter { it.favourite }
    val visible = rows.filter { "${it.title} ${it.artist} ${it.album}".contains(query, true) }
    LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(bottom = 24.dp)) {
        item(key = "header") {
            CollectionHeader(
                "Favourites",
                collectionSummary(rows),
                { FavouritesArtwork(Modifier.size(176.dp)) },
                { play(rows, 0, false) },
                { play(rows, rows.indices.random(), true) },
                rows.isNotEmpty(),
            )
        }
        if (rows.isEmpty())
            item(key = "empty") {
                CollectionMessage(
                    "Keep the songs you love together. Tap a heart on any song or on the player to add it here."
                )
                Button(onClick = browse, modifier = Modifier.padding(horizontal = 20.dp)) {
                    Text("Browse songs")
                }
            }
        else {
            item(key = "search") {
                OutlinedTextField(
                    query,
                    { query = it },
                    placeholder = { Text("Search favourites") },
                    leadingIcon = { Icon(Icons.Default.Search, null) },
                    singleLine = true,
                    shape = RoundedCornerShape(20.dp),
                    modifier =
                        Modifier.fillMaxWidth().padding(horizontal = 20.dp, vertical = 12.dp),
                )
            }
            itemsIndexed(visible, key = { _, track -> track.id.toString() }) { index, track ->
                CollectionTrackRow(
                    track,
                    index,
                    library,
                    snapshot?.current?.id == track.id,
                    { play(visible, index, false) },
                    modifier = Modifier.animateItem(),
                ) {
                    FavouriteButton(track, library)
                }
            }
            if (visible.isEmpty())
                item(key = "no-results") { CollectionMessage("No favourites match “$query”") }
        }
    }
}

@Composable
fun AlbumScreen(
    library: LibraryRepository,
    title: String,
    artist: String,
    play: (List<Track>, Int, Boolean) -> Unit,
    navigate: (String) -> Unit,
) {
    val tracks by library.allTracks.collectAsState()
    val snapshot by library.snapshot.collectAsState()
    val rows =
        tracks
            .filter { it.album == title && it.albumArtist.ifBlank { it.artist } == artist }
            .sortedWith(
                compareBy<Track> { it.discNumber ?: 1u }
                    .thenBy { it.trackNumber ?: UInt.MAX_VALUE }
                    .thenBy { it.title }
            )
    LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(bottom = 24.dp)) {
        item(key = "header") {
            CollectionHeader(
                title.ifBlank { "Unknown album" },
                listOfNotNull(
                        rows.firstOrNull()?.date?.takeIf { it.isNotBlank() },
                        collectionSummary(rows),
                    )
                    .joinToString(" · "),
                { Artwork(Modifier.size(208.dp), rows.firstOrNull(), library) },
                { play(rows, 0, false) },
                { play(rows, rows.indices.random(), true) },
                rows.isNotEmpty(),
                track = rows.firstOrNull(),
                library = library,
                detail = {
                    TextButton(
                        onClick = { navigate(artistDestination(artist)) },
                        enabled = artist.isNotBlank(),
                    ) {
                        Text(
                            artist.ifBlank { "Unknown artist" },
                            style = MaterialTheme.typography.titleMedium,
                        )
                    }
                },
            )
        }
        if (rows.isEmpty())
            item(key = "empty") {
                CollectionMessage("No songs from this album are in your library yet.")
            }
        itemsIndexed(rows, key = { _, track -> track.id.toString() }) { index, track ->
            CollectionTrackRow(
                track,
                index,
                library,
                snapshot?.current?.id == track.id,
                { play(rows, index, false) },
                modifier = Modifier.animateItem(),
            ) {
                FavouriteButton(track, library)
            }
        }
    }
}

@Composable
fun ArtistScreen(
    library: LibraryRepository,
    name: String,
    play: (List<Track>, Int, Boolean) -> Unit,
    navigate: (String) -> Unit,
) {
    val tracks by library.allTracks.collectAsState()
    val snapshot by library.snapshot.collectAsState()
    val rows = tracks.filter { it.artist.equals(name, true) || it.albumArtist.equals(name, true) }
    val albums = rows.groupBy { it.album to it.albumArtist.ifBlank { it.artist } }.entries.toList()
    LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(bottom = 24.dp)) {
        item(key = "header") {
            CollectionHeader(
                name.ifBlank { "Unknown artist" },
                "${albums.size} ${if (albums.size == 1) "album" else "albums"} · ${songCount(rows.size)}",
                { Artwork(Modifier.size(208.dp), rows.firstOrNull(), library, CircleShape) },
                { play(rows, 0, false) },
                { play(rows, rows.indices.random(), true) },
                rows.isNotEmpty(),
                track = rows.firstOrNull(),
                library = library,
                artworkShape = CircleShape,
            )
        }
        item(key = "albums") { CollectionHeading("Albums") }
        items(albums, key = { "album:${it.key.first.length}:${it.key.first}${it.key.second}" }) {
            album ->
            AlbumRow(album.value, library, Modifier.animateItem()) {
                navigate(albumDestination(album.key.first, album.key.second))
            }
        }
        item(key = "songs") { CollectionHeading("Songs") }
        itemsIndexed(rows, key = { _, track -> "song:${track.id}" }) { index, track ->
            CollectionTrackRow(
                track,
                index,
                library,
                snapshot?.current?.id == track.id,
                { play(rows, index, false) },
                modifier = Modifier.animateItem(),
            ) {
                FavouriteButton(track, library)
            }
        }
        if (rows.isEmpty())
            item(key = "empty") {
                CollectionMessage("No music by this artist is in your library yet.")
            }
    }
}

fun songCount(count: Int) = "$count ${if (count == 1) "song" else "songs"}"

fun collectionSummary(rows: List<Track>): String {
    val minutes = rows.sumOf { it.durationMs }.toLong() / 60000
    return "${songCount(rows.size)} · ${if (minutes >= 60) "${minutes / 60} hr ${minutes % 60} min" else "$minutes min"}"
}
