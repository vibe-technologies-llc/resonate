package org.resonate.android

import android.net.Uri
import androidx.activity.compose.BackHandler
import androidx.compose.animation.Crossfade
import androidx.compose.animation.core.tween
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.Image
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.QueueMusic
import androidx.compose.material.icons.filled.Album
import androidx.compose.material.icons.filled.Bedtime
import androidx.compose.material.icons.filled.Equalizer
import androidx.compose.material.icons.filled.Favorite
import androidx.compose.material.icons.filled.FavoriteBorder
import androidx.compose.material.icons.filled.FolderOpen
import androidx.compose.material.icons.filled.Info
import androidx.compose.material.icons.filled.LibraryMusic
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.MusicNote
import androidx.compose.material.icons.filled.Pause
import androidx.compose.material.icons.filled.Person
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.Repeat
import androidx.compose.material.icons.filled.RepeatOne
import androidx.compose.material.icons.filled.Search
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.filled.Shuffle
import androidx.compose.material.icons.filled.SkipNext
import androidx.compose.material.icons.filled.SkipPrevious
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilledIconButton
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Slider
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.saveable.rememberSaveableStateHolder
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.media3.session.MediaController
import org.resonate.android.nativecore.Snapshot
import org.resonate.android.nativecore.Track
import org.resonate.android.nativecore.Transport

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ResonateScreen(
    library: LibraryRepository,
    controller: MediaController?,
    chooseFolder: () -> Unit,
    allowNotifications: () -> Unit,
) {
    val tracks by library.tracks.collectAsState()
    val queue by library.queue.collectAsState()
    val snapshot by library.snapshot.collectAsState()
    val scanning by library.scanning.collectAsState()
    val status by library.status.collectAsState()
    var page by rememberSaveable { mutableStateOf("Library") }
    var activeTab by rememberSaveable { mutableStateOf("Library") }
    var history by rememberSaveable { mutableStateOf<List<String>>(emptyList()) }
    val savedPages = rememberSaveableStateHolder()
    var filter by remember { mutableStateOf("Songs") }
    var search by remember { mutableStateOf("") }
    var nowPlaying by rememberSaveable { mutableStateOf(false) }
    var sleepTimer by remember { mutableStateOf(false) }
    var filters by remember { mutableStateOf(false) }
    var selectedTrack by remember { mutableStateOf<Track?>(null) }
    val featureTrack =
        selectedTrack?.let { selected -> tracks.find { it.id == selected.id } ?: selected }
            ?: snapshot?.current
    val section = page.substringBefore('?')
    val route = Uri.parse(page)
    val navigate: (String) -> Unit = { destination ->
        if (destination == "Settings") {
            history = emptyList()
            activeTab = "Settings"
        } else if (destination != page || nowPlaying)
            history = history + if (nowPlaying) "Now playing:$page" else page
        page = destination
        nowPlaying = false
    }
    val back: () -> Unit = {
        when {
            nowPlaying -> nowPlaying = false
            history.isNotEmpty() -> {
                val previous = history.last()
                history = history.dropLast(1)
                nowPlaying = previous.startsWith("Now playing:")
                page = previous.removePrefix("Now playing:")
            }
            else -> {
                page = "Library"
                activeTab = "Library"
            }
        }
    }
    val playCollection: (List<Track>, Int, Boolean) -> Unit = { rows, index, shuffle ->
        if (rows.isNotEmpty()) {
            controller?.setMediaItems(rows.map { it.mediaItem() }, index.coerceIn(rows.indices), 0)
            controller?.shuffleModeEnabled = shuffle
            controller?.prepare()
            controller?.play()
        }
    }
    val playTracks: (List<Track>, Track) -> Unit = { rows, selected ->
        controller?.setMediaItems(
            rows.map { it.mediaItem() },
            rows.indexOf(selected).coerceAtLeast(0),
            0,
        )
        controller?.prepare()
        controller?.play()
    }
    BackHandler(enabled = nowPlaying || page != "Library") { back() }
    MaterialTheme(
        colorScheme =
            if (isSystemInDarkTheme()) dynamicDarkColorScheme(LocalContext.current)
            else dynamicLightColorScheme(LocalContext.current)
    ) {
        Scaffold(
            topBar = {
                TopAppBar(
                    title = {
                        Text(
                            if (nowPlaying) "Now playing"
                            else
                                when (section) {
                                    "Library" -> "Your library"
                                    "Songs" -> "Resonate"
                                    "Sound" -> "Equaliser"
                                    "Processing" -> "Sound processing"
                                    "Tools" -> "Library tools"
                                    "Playlist" -> "Playlist"
                                    "Album" -> "Album"
                                    "Artist" -> "Artist"
                                    else -> section
                                },
                            fontWeight = FontWeight.SemiBold,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                    },
                    navigationIcon = {
                        if (
                            nowPlaying ||
                                section !in listOf("Library", "Songs", "Queue", "Settings")
                        )
                            IconButton(onClick = back) {
                                Icon(Icons.AutoMirrored.Filled.ArrowBack, "Back")
                            }
                    },
                    actions = {
                        if (activeTab != "Settings") {
                            if (snapshot?.current != null)
                                IconButton(onClick = { sleepTimer = true }) {
                                    Icon(
                                        Icons.Default.Bedtime,
                                        "Sleep timer",
                                        tint =
                                            if (snapshot?.sleep != null)
                                                MaterialTheme.colorScheme.primary
                                            else MaterialTheme.colorScheme.onSurfaceVariant,
                                    )
                                }
                            if (section in listOf("Library", "Songs"))
                                IconButton(onClick = chooseFolder, enabled = !scanning) {
                                    Icon(Icons.Default.FolderOpen, "Add music folder")
                                }
                            if (page != "Tools")
                                IconButton(
                                    onClick = {
                                        selectedTrack = null
                                        navigate("Tools")
                                    }
                                ) {
                                    Icon(Icons.Default.MoreVert, "Library tools")
                                }
                        }
                    },
                )
            },
            bottomBar = {
                Column {
                    if (!nowPlaying && snapshot?.current != null)
                        MiniPlayer(snapshot!!, controller, library, { nowPlaying = true })
                    NavigationBar {
                        listOf(
                                "Songs" to Icons.Default.MusicNote,
                                "Library" to Icons.Default.LibraryMusic,
                                "Queue" to Icons.AutoMirrored.Filled.QueueMusic,
                                "Settings" to Icons.Default.Settings,
                            )
                            .forEach { (title, icon) ->
                                NavigationBarItem(
                                    selected = activeTab == title && !nowPlaying,
                                    onClick = {
                                        page = title
                                        activeTab = title
                                        history = emptyList()
                                        nowPlaying = false
                                    },
                                    icon = { Icon(icon, title) },
                                    label = { Text(title) },
                                )
                            }
                    }
                }
            },
        ) { padding ->
            Column(Modifier.fillMaxSize().padding(padding)) {
                if (scanning) LinearProgressIndicator(Modifier.fillMaxWidth())
                status?.let { message ->
                    Surface(
                        color = MaterialTheme.colorScheme.surfaceContainer,
                        modifier = Modifier.fillMaxWidth(),
                    ) {
                        Row(
                            Modifier.padding(start = 16.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Text(
                                message,
                                Modifier.weight(1f),
                                style = MaterialTheme.typography.bodySmall,
                            )
                            TextButton(onClick = library::dismissStatus) { Text("Dismiss") }
                        }
                    }
                }
                PageReveal(page to nowPlaying, Modifier.weight(1f).fillMaxWidth()) {
                    if (nowPlaying) {
                        NowPlaying(
                            snapshot,
                            controller,
                            library,
                            openTools = {
                                selectedTrack = null
                                navigate(it)
                            },
                            openSound = { navigate("Sound") },
                            openSleep = { sleepTimer = true },
                            openAlbum = { track ->
                                navigate(
                                    albumDestination(
                                        track.album,
                                        track.albumArtist.ifBlank { track.artist },
                                    )
                                )
                            },
                            openArtist = { name -> navigate(artistDestination(name)) },
                        )
                    } else
                        when (section) {
                            "Settings" ->
                                savedPages.SaveableStateProvider("Settings") {
                                    SettingsScreen(
                                        library,
                                        snapshot,
                                        navigate,
                                        openSleep = { sleepTimer = true },
                                        allowNotifications,
                                    )
                                }
                            "Tools" ->
                                LibraryToolsScreen(
                                    library,
                                    featureTrack != null,
                                    navigate,
                                    openFilters = { filters = true },
                                )
                            "Processing" -> ProcessingSettings(library)
                            "Sound" -> SoundSettings(library)
                            "Library" ->
                                savedPages.SaveableStateProvider("Library") {
                                    LibraryScreen(library, navigate)
                                }
                            "Playlists" -> LibraryScreen(library, navigate, playlistsOnly = true)
                            "Playlist" ->
                                route.getQueryParameter("id")?.toULongOrNull()?.let { id ->
                                    PlaylistScreen(
                                        library,
                                        id,
                                        playCollection,
                                        back,
                                        inspect = {
                                            selectedTrack = it
                                            navigate("Inspector")
                                        },
                                    )
                                }
                            "Favourites" ->
                                FavouritesScreen(
                                    library,
                                    playCollection,
                                    browse = { navigate("Songs") },
                                )
                            "Album" ->
                                AlbumScreen(
                                    library,
                                    route.getQueryParameter("title").orEmpty(),
                                    route.getQueryParameter("artist").orEmpty(),
                                    playCollection,
                                    navigate,
                                )
                            "Artist" ->
                                ArtistScreen(
                                    library,
                                    route.getQueryParameter("name").orEmpty(),
                                    playCollection,
                                    navigate,
                                )
                            "Folders" -> FolderScreen(library, chooseFolder)
                            "Suggestions" -> SuggestionsScreen(library) { navigate("Songs") }
                            "Statistics" -> StatisticsScreen(library, playTracks)
                            "Online search" ->
                                OnlineScreen(library, openSettings = { navigate("Settings") })
                            "Saved for later" -> WantedScreen(library)
                            "Download providers" -> ProviderScreen(library)
                            "Scrobbling" -> ScrobblingScreen(library)
                            "AutoEq" -> AutoEqScreen(library)
                            "Output" -> OutputScreen(library, snapshot)
                            "Lyrics" -> LyricsScreen(library, featureTrack, snapshot, controller)
                            "Visualizer" -> VisualizerScreen(library, snapshot)
                            "Analysis" -> AnalysisScreen(library, featureTrack)
                            "Inspector" -> InspectorScreen(library, featureTrack)
                            "Queue" ->
                                SongList(
                                    library,
                                    {
                                        selectedTrack = it
                                        navigate("Inspector")
                                    },
                                    queue,
                                    snapshot?.current?.id,
                                    emptyText = "Your queue is empty",
                                    play = { selected ->
                                        controller?.seekToDefaultPosition(
                                            queue.indexOfFirst { it.queueId == selected.queueId }
                                        )
                                        controller?.play()
                                    },
                                    favourite = { track ->
                                        library.setFavourite(track.id, !track.favourite)
                                    },
                                )
                            else -> {
                                OutlinedTextField(
                                    value = search,
                                    onValueChange = {
                                        search = it
                                        library.search(it)
                                    },
                                    singleLine = true,
                                    placeholder = { Text("Search your music") },
                                    leadingIcon = { Icon(Icons.Default.Search, null) },
                                    modifier =
                                        Modifier.fillMaxWidth()
                                            .padding(horizontal = 16.dp, vertical = 8.dp),
                                    shape = RoundedCornerShape(16.dp),
                                )
                                Row(
                                    Modifier.padding(horizontal = 16.dp),
                                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                                ) {
                                    listOf("Songs", "Albums", "Artists").forEach { title ->
                                        FilterChip(
                                            selected = filter == title,
                                            onClick = { filter = title },
                                            label = { Text(title) },
                                        )
                                    }
                                }
                                Row(
                                    Modifier.padding(horizontal = 16.dp)
                                        .horizontalScroll(rememberScrollState()),
                                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                                ) {
                                    listOf("Playlists", "Suggestions", "Online search").forEach {
                                        destination ->
                                        TextButton(onClick = { navigate(destination) }) {
                                            Text(destination)
                                        }
                                    }
                                    TextButton(onClick = { filters = true }) { Text("Filters") }
                                }
                                val visible = tracks
                                if (tracks.isEmpty() && !scanning && search.isEmpty()) {
                                    Column(
                                        Modifier.fillMaxSize().padding(32.dp),
                                        horizontalAlignment = Alignment.CenterHorizontally,
                                        verticalArrangement = Arrangement.Center,
                                    ) {
                                        Icon(
                                            Icons.Default.LibraryMusic,
                                            null,
                                            Modifier.size(64.dp),
                                            tint = MaterialTheme.colorScheme.primary,
                                        )
                                        Spacer(Modifier.height(20.dp))
                                        Text(
                                            "Your music, in its own detail",
                                            style = MaterialTheme.typography.headlineSmall,
                                        )
                                        Spacer(Modifier.height(12.dp))
                                        Text(
                                            "Choose a music folder to build your library. Your files stay where they are.",
                                            style = MaterialTheme.typography.bodyMedium,
                                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                                        )
                                        Spacer(Modifier.height(24.dp))
                                        Button(onClick = chooseFolder) {
                                            Text("Choose music folder")
                                        }
                                    }
                                } else if (filter != "Songs") {
                                    val groups = visible.groupBy {
                                        if (filter == "Artists") it.artist to ""
                                        else it.album to it.albumArtist.ifBlank { it.artist }
                                    }
                                    LazyColumn {
                                        items(
                                            groups.entries.toList(),
                                            key = {
                                                "${it.key.first.length}:${it.key.first}${it.key.second}"
                                            },
                                        ) { group ->
                                            if (filter == "Artists")
                                                CollectionRow(
                                                    group.key.first.ifBlank { "Unknown artist" },
                                                    "Artist · ${songCount(group.value.size)}",
                                                    artwork = {
                                                        Artwork(
                                                            Modifier.size(64.dp),
                                                            group.value.first(),
                                                            library,
                                                            androidx.compose.foundation.shape
                                                                .CircleShape,
                                                        )
                                                    },
                                                ) {
                                                    navigate(artistDestination(group.key.first))
                                                }
                                            else
                                                AlbumRow(group.value, library) {
                                                    navigate(
                                                        albumDestination(
                                                            group.key.first,
                                                            group.key.second,
                                                        )
                                                    )
                                                }
                                        }
                                    }
                                } else {
                                    SongList(
                                        library,
                                        {
                                            selectedTrack = it
                                            navigate("Inspector")
                                        },
                                        visible,
                                        snapshot?.current?.id,
                                        "No songs match",
                                        play = { selected ->
                                            controller?.setMediaItems(
                                                visible.map { it.mediaItem() },
                                                visible.indexOfFirst { it.id == selected.id },
                                                0,
                                            )
                                            controller?.prepare()
                                            controller?.play()
                                        },
                                        favourite = { track ->
                                            library.setFavourite(track.id, !track.favourite)
                                        },
                                    )
                                }
                            }
                        }
                }
            }
        }
        if (filters) LibraryFilterDialog(library) { filters = false }
        if (sleepTimer) SleepTimerDialog(snapshot, library, dismiss = { sleepTimer = false })
    }
}

@Composable
fun Artwork(
    modifier: Modifier = Modifier,
    track: Track? = null,
    library: LibraryRepository? = null,
    shape: Shape = RoundedCornerShape(12.dp),
) {
    val generation = library?.features?.artworkGeneration?.collectAsState()?.value
    val bitmap by
        produceState<android.graphics.Bitmap?>(null, track?.id, track?.artRevision, generation) {
            value = if (track != null && library != null) library.features.artwork(track) else null
        }
    Surface(modifier, shape = shape, color = MaterialTheme.colorScheme.secondaryContainer) {
        Crossfade(targetState = bitmap, animationSpec = tween(360), label = "Artwork handover") {
            picture ->
            if (picture != null) {
                Image(
                    picture.asImageBitmap(),
                    contentDescription = null,
                    modifier = Modifier.fillMaxSize(),
                    contentScale = ContentScale.Crop,
                )
            } else {
                Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                    Icon(
                        Icons.Default.MusicNote,
                        null,
                        Modifier.size(32.dp),
                        tint = MaterialTheme.colorScheme.primary,
                    )
                }
            }
        }
    }
}

@Composable
private fun SongList(
    library: LibraryRepository,
    inspect: (Track) -> Unit,
    tracks: List<Track>,
    current: ULong?,
    emptyText: String,
    play: (Track) -> Unit,
    favourite: (Track) -> Unit,
) {
    if (tracks.isEmpty()) {
        Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
            Text(emptyText, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    } else
        LazyColumn {
            itemsIndexed(tracks, key = { index, track -> "$index:${track.id}" }) { _, track ->
                Row(
                    Modifier.fillMaxWidth()
                        .clickable { play(track) }
                        .padding(horizontal = 16.dp, vertical = 10.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Artwork(Modifier.size(48.dp), track, library)
                    Column(Modifier.weight(1f).padding(horizontal = 12.dp)) {
                        Text(
                            track.title,
                            style = MaterialTheme.typography.titleSmall,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                            color =
                                if (current == track.id) MaterialTheme.colorScheme.primary
                                else MaterialTheme.colorScheme.onSurface,
                        )
                        Text(
                            track.artist.ifBlank { "Unknown artist" },
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                    }
                    IconButton(onClick = { inspect(track) }) {
                        Icon(Icons.Default.Info, "Inspect ${track.title}", Modifier.size(20.dp))
                    }
                    IconButton(onClick = { favourite(track) }) {
                        Icon(
                            if (track.favourite) Icons.Default.Favorite
                            else Icons.Default.FavoriteBorder,
                            if (track.favourite) "Remove favourite" else "Favourite",
                            Modifier.size(20.dp),
                        )
                    }
                }
            }
        }
}

@Composable
private fun MiniPlayer(
    snapshot: Snapshot,
    controller: MediaController?,
    library: LibraryRepository,
    open: () -> Unit,
) {
    val tones by rememberArtworkTones(snapshot.current, library)
    Surface(
        shape = RoundedCornerShape(28.dp),
        border = BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.5f)),
        color = MaterialTheme.colorScheme.surfaceContainerHigh,
        tonalElevation = 6.dp,
        shadowElevation = 6.dp,
        modifier = Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 8.dp),
    ) {
        AmbientBackdrop(
            tones,
            strength = 0.12f,
            base = MaterialTheme.colorScheme.surfaceContainerHigh,
        ) {
            Column(
                Modifier.clickable(onClickLabel = "Open now playing", onClick = open)
                    .padding(horizontal = 12.dp, vertical = 10.dp)
            ) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Artwork(Modifier.size(48.dp), snapshot.current, library)
                    Crossfade(
                        targetState =
                            PlayerLabel(
                                snapshot.current?.id,
                                snapshot.current?.title.orEmpty(),
                                snapshot.current?.artist.orEmpty(),
                            ),
                        animationSpec = tween(220),
                        modifier = Modifier.weight(1f).padding(start = 12.dp, end = 4.dp),
                        label = "Mini player song handover",
                    ) { label ->
                        Column {
                            Text(
                                label.title,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                                style = MaterialTheme.typography.titleSmall,
                            )
                            Text(
                                label.artist.ifBlank { "Unknown artist" },
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                    }
                    FavouriteButton(snapshot.current, library, current = true)
                }
                Row(
                    Modifier.fillMaxWidth().padding(top = 4.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Column(Modifier.weight(1f).padding(end = 12.dp)) {
                        LinearProgressIndicator(
                            progress = {
                                (snapshot.positionMs.toFloat() /
                                        snapshot.durationMs.coerceAtLeast(1uL).toFloat())
                                    .coerceIn(0f, 1f)
                            },
                            modifier =
                                Modifier.fillMaxWidth().semantics {
                                    contentDescription = "Song progress"
                                },
                        )
                        Row(
                            Modifier.fillMaxWidth().padding(top = 4.dp),
                            horizontalArrangement = Arrangement.SpaceBetween,
                        ) {
                            Text(
                                clock(snapshot.positionMs),
                                style = MaterialTheme.typography.labelSmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                            Text(
                                clock(snapshot.durationMs),
                                style = MaterialTheme.typography.labelSmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                    }
                    IconButton(
                        onClick = { controller?.seekToPreviousMediaItem() },
                        enabled = controller != null,
                    ) {
                        Icon(Icons.Default.SkipPrevious, "Previous song")
                    }
                    PlayButton(snapshot, controller)
                    IconButton(
                        onClick = { controller?.seekToNextMediaItem() },
                        enabled = controller != null,
                    ) {
                        Icon(Icons.Default.SkipNext, "Next song")
                    }
                }
            }
        }
    }
}

@Composable
private fun PlayButton(
    snapshot: Snapshot?,
    controller: MediaController?,
    expanded: Boolean = false,
) {
    val playing =
        snapshot?.transport == Transport.PLAYING || snapshot?.transport == Transport.LOADING
    FilledIconButton(
        onClick = { if (playing) controller?.pause() else controller?.play() },
        enabled = controller != null,
        modifier =
            Modifier.size(if (expanded) 64.dp else 48.dp).semantics {
                contentDescription = if (playing) "Pause" else "Play"
            },
    ) {
        Crossfade(playing, animationSpec = tween(160), label = "Play pause icon") { active ->
            Icon(
                if (active) Icons.Default.Pause else Icons.Default.PlayArrow,
                null,
                Modifier.size(if (expanded) 32.dp else 24.dp),
            )
        }
    }
}

@Immutable private data class PlayerLabel(val id: ULong?, val title: String, val artist: String)

@Composable
private fun NowPlaying(
    snapshot: Snapshot?,
    controller: MediaController?,
    library: LibraryRepository,
    openTools: (String) -> Unit,
    openSound: () -> Unit,
    openSleep: () -> Unit,
    openAlbum: (Track) -> Unit,
    openArtist: (String) -> Unit,
) {
    val track = snapshot?.current
    val tones by rememberArtworkTones(track, library)
    val playing =
        snapshot?.transport == Transport.PLAYING || snapshot?.transport == Transport.LOADING
    AmbientBackdrop(tones, Modifier.fillMaxSize(), strength = 0.4f) {
        Column(
            Modifier.fillMaxWidth()
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 24.dp, vertical = 20.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            BoxWithConstraints(Modifier.fillMaxWidth(), contentAlignment = Alignment.Center) {
                val edge = minOf(maxWidth - 32.dp, 264.dp)
                ArtworkSpotlight(Modifier.size(edge), playing) {
                    Artwork(Modifier.fillMaxSize(), track, library, RoundedCornerShape(24.dp))
                }
            }
            Spacer(Modifier.height(20.dp))
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                Text(
                    track?.title ?: "Nothing playing",
                    Modifier.weight(1f),
                    style = MaterialTheme.typography.headlineSmall,
                    fontWeight = FontWeight.SemiBold,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                )
                FavouriteButton(track, library, current = true)
            }
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                TextButton(
                    onClick = { track?.let { openArtist(it.artist) } },
                    enabled = !track?.artist.isNullOrBlank(),
                    modifier =
                        Modifier.weight(1f).semantics {
                            contentDescription = "Open artist ${track?.artist.orEmpty()}"
                        },
                ) {
                    Icon(Icons.Default.Person, null, Modifier.size(18.dp))
                    Text(
                        track?.artist?.ifBlank { "Unknown artist" }.orEmpty(),
                        Modifier.padding(start = 6.dp),
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
                TextButton(
                    onClick = { track?.let(openAlbum) },
                    enabled = !track?.album.isNullOrBlank(),
                    modifier =
                        Modifier.weight(1f).semantics {
                            contentDescription = "Open album ${track?.album.orEmpty()}"
                        },
                ) {
                    Icon(Icons.Default.Album, null, Modifier.size(18.dp))
                    Text(
                        track?.album.orEmpty(),
                        Modifier.padding(start = 6.dp),
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
            }
            var seeking by remember { mutableStateOf(false) }
            var position by remember { mutableFloatStateOf(0f) }
            val duration = snapshot?.durationMs?.toFloat()?.coerceAtLeast(1f) ?: 1f
            Slider(
                value =
                    if (seeking) position
                    else (snapshot?.positionMs?.toFloat() ?: 0f).coerceIn(0f, duration),
                onValueChange = {
                    seeking = true
                    position = it
                },
                onValueChangeFinished = {
                    controller?.seekTo(position.toLong())
                    seeking = false
                },
                valueRange = 0f..duration,
            )
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
                Text(
                    clock(snapshot?.positionMs ?: 0uL),
                    style = MaterialTheme.typography.labelSmall,
                )
                Text(
                    clock(snapshot?.durationMs ?: 0uL),
                    style = MaterialTheme.typography.labelSmall,
                )
            }
            Row(
                Modifier.fillMaxWidth().padding(vertical = 12.dp),
                horizontalArrangement = Arrangement.SpaceEvenly,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                IconButton(
                    onClick = { controller?.shuffleModeEnabled = !(snapshot?.shuffle ?: false) }
                ) {
                    Icon(
                        Icons.Default.Shuffle,
                        "Shuffle",
                        tint =
                            if (snapshot?.shuffle == true) MaterialTheme.colorScheme.primary
                            else MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                IconButton(onClick = { controller?.seekToPreviousMediaItem() }) {
                    Icon(Icons.Default.SkipPrevious, "Previous song")
                }
                PlayButton(snapshot, controller, expanded = true)
                IconButton(onClick = { controller?.seekToNextMediaItem() }) {
                    Icon(Icons.Default.SkipNext, "Next song")
                }
                IconButton(
                    onClick = {
                        controller?.repeatMode = ((snapshot?.repeat?.toInt() ?: 0) + 1) % 3
                    }
                ) {
                    Icon(
                        if (snapshot?.repeat == 1u) Icons.Default.RepeatOne
                        else Icons.Default.Repeat,
                        "Repeat",
                        tint =
                            if (snapshot?.repeat != 0u) MaterialTheme.colorScheme.primary
                            else MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                TextButton(onClick = openSound) {
                    Icon(Icons.Default.Equalizer, null)
                    Text("Equaliser")
                }
                TextButton(onClick = openSleep) {
                    Icon(Icons.Default.Bedtime, null)
                    Text("Sleep timer")
                }
            }
            Row(Modifier.horizontalScroll(rememberScrollState())) {
                listOf("Lyrics", "Visualizer", "Analysis", "Inspector").forEach { destination ->
                    TextButton(onClick = { openTools(destination) }) { Text(destination) }
                }
            }
            if (snapshot?.sleep != null)
                Text(
                    sleepDescription(snapshot),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.primary,
                )
            track?.let {
                val sourceRate = (it.rate.toDouble() / 1000).toString().removeSuffix(".0")
                val outputRate =
                    snapshot.outputRate
                        .takeIf { rate -> rate > 0u }
                        ?.let { rate ->
                            "${(rate.toDouble() / 1000).toString().removeSuffix(".0")} kHz output"
                        }
                Text(
                    listOfNotNull("${it.codec} · $sourceRate kHz source", outputRate)
                        .joinToString(" · "),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}

private fun clock(milliseconds: ULong): String {
    val seconds = milliseconds / 1000u
    return "${seconds / 60u}:${(seconds % 60u).toString().padStart(2, '0')}"
}
