package org.resonate.android

import android.net.Uri
import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.Crossfade
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutVertically
import androidx.compose.animation.core.Spring
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.spring
import androidx.compose.animation.core.tween
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.Image
import androidx.compose.foundation.basicMarquee
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.isSystemInDarkTheme
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
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.QueueMusic
import androidx.compose.material.icons.filled.Album
import androidx.compose.material.icons.filled.Bedtime
import androidx.compose.material.icons.filled.Clear
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Equalizer
import androidx.compose.material.icons.filled.Favorite
import androidx.compose.material.icons.filled.FavoriteBorder
import androidx.compose.material.icons.filled.FolderOpen
import androidx.compose.material.icons.filled.GraphicEq
import androidx.compose.material.icons.filled.Info
import androidx.compose.material.icons.filled.Insights
import androidx.compose.material.icons.filled.LibraryMusic
import androidx.compose.material.icons.filled.Lyrics
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
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.FilledTonalIconButton
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.IconButtonDefaults
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.OutlinedTextFieldDefaults
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Slider
import androidx.compose.material3.SliderDefaults
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.LaunchedEffect
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
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.media3.session.MediaController
import kotlinx.coroutines.delay
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
    var likedTrackForBanner by remember { mutableStateOf<Track?>(null) }
    var playlistPickerTrack by remember { mutableStateOf<Track?>(null) }
    LaunchedEffect(likedTrackForBanner) {
        if (likedTrackForBanner != null) {
            delay(4000)
            likedTrackForBanner = null
        }
    }
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
        CompositionLocalProvider(
            LocalOnLiked provides { track -> likedTrackForBanner = track }
        ) {
            Box(Modifier.fillMaxSize()) {
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
                            fontWeight = FontWeight.Bold,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                    },
                    colors =
                        TopAppBarDefaults.topAppBarColors(
                            containerColor = MaterialTheme.colorScheme.surfaceContainerLow
                        ),
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
                    NavigationBar(
                        containerColor = MaterialTheme.colorScheme.surfaceContainer,
                        tonalElevation = 4.dp,
                    ) {
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
                                    label = { Text(title, fontWeight = FontWeight.Medium) },
                                )
                            }
                    }
                }
            },
        ) { padding ->
            Column(Modifier.fillMaxSize().padding(padding)) {
                if (scanning)
                    LinearProgressIndicator(
                        Modifier.fillMaxWidth(),
                        strokeCap = StrokeCap.Round,
                    )
                status?.let { message ->
                    Surface(
                        color = MaterialTheme.colorScheme.surfaceContainerHigh,
                        shape = RoundedCornerShape(16.dp),
                        border =
                            BorderStroke(
                                1.dp,
                                MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.35f),
                            ),
                        modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 6.dp),
                    ) {
                        Row(
                            Modifier.padding(start = 16.dp, end = 8.dp, top = 4.dp, bottom = 4.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Text(
                                message,
                                Modifier.weight(1f),
                                style = MaterialTheme.typography.bodyMedium,
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
                            "Device sync" -> SyncScreen(library)
                            "AutoEq" -> AutoEqScreen(library)
                            "Output" -> OutputScreen(library, snapshot)
                            "Lyrics" -> LyricsScreen(library, featureTrack, snapshot, controller)
                            "Visualizer" -> VisualizerScreen(library, snapshot)
                            "Analysis" -> AnalysisScreen(library, featureTrack)
                            "Inspector" -> InspectorScreen(library, featureTrack)
                            "Queue" ->
                                Column(Modifier.fillMaxSize()) {
                                    Surface(
                                        color = MaterialTheme.colorScheme.surfaceContainerLow,
                                        modifier = Modifier.fillMaxWidth(),
                                    ) {
                                        Row(
                                            Modifier.fillMaxWidth().padding(horizontal = 20.dp, vertical = 12.dp),
                                            verticalAlignment = Alignment.CenterVertically,
                                            horizontalArrangement = Arrangement.SpaceBetween,
                                        ) {
                                            Text(
                                                "Up next · ${songCount(queue.size)}",
                                                style = MaterialTheme.typography.titleMedium,
                                                fontWeight = FontWeight.SemiBold,
                                            )
                                            if (queue.isNotEmpty())
                                                Text(
                                                    collectionSummary(queue),
                                                    style = MaterialTheme.typography.bodySmall,
                                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                                )
                                        }
                                    }
                                    HandedOffRow(library) { controller?.play() }
                                    Box(Modifier.weight(1f)) {
                                        SongList(
                                            library,
                                            {
                                                selectedTrack = it
                                                navigate("Inspector")
                                            },
                                            queue,
                                            snapshot?.current?.id,
                                            playing = snapshot?.transport == Transport.PLAYING,
                                            emptyText = "Your queue is empty",
                                            play = { selected ->
                                                controller?.seekToDefaultPosition(
                                                    queue.indexOfFirst {
                                                        it.queueId == selected.queueId
                                                    }
                                                )
                                                controller?.play()
                                            },
                                            favourite = { track ->
                                                library.setFavourite(track.id, !track.favourite)
                                            },
                                        )
                                    }
                                }
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
                                    trailingIcon = {
                                        if (search.isNotEmpty())
                                            IconButton(
                                                onClick = {
                                                    search = ""
                                                    library.search("")
                                                }
                                            ) {
                                                Icon(Icons.Default.Clear, "Clear search")
                                            }
                                    },
                                    colors =
                                        OutlinedTextFieldDefaults.colors(
                                            focusedContainerColor =
                                                MaterialTheme.colorScheme.surfaceContainerHigh,
                                            unfocusedContainerColor =
                                                MaterialTheme.colorScheme.surfaceContainerHigh,
                                            focusedBorderColor = MaterialTheme.colorScheme.primary,
                                            unfocusedBorderColor = Color.Transparent,
                                        ),
                                    modifier =
                                        Modifier.fillMaxWidth()
                                            .padding(horizontal = 16.dp, vertical = 8.dp),
                                    shape = RoundedCornerShape(28.dp),
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
                                            shape = RoundedCornerShape(16.dp),
                                        )
                                    }
                                }
                                Row(
                                    Modifier.padding(horizontal = 16.dp, vertical = 4.dp)
                                        .horizontalScroll(rememberScrollState()),
                                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                                ) {
                                    listOf("Playlists", "Suggestions", "Online search").forEach {
                                        destination ->
                                        FilledTonalButton(
                                            onClick = { navigate(destination) },
                                            contentPadding = PaddingValues(horizontal = 14.dp, vertical = 4.dp),
                                            shape = RoundedCornerShape(14.dp),
                                        ) {
                                            Text(destination, style = MaterialTheme.typography.labelLarge)
                                        }
                                    }
                                    FilledTonalButton(
                                        onClick = { filters = true },
                                        contentPadding = PaddingValues(horizontal = 14.dp, vertical = 4.dp),
                                        shape = RoundedCornerShape(14.dp),
                                    ) {
                                        Text("Filters", style = MaterialTheme.typography.labelLarge)
                                    }
                                }
                                val visible = tracks
                                if (tracks.isEmpty() && !scanning && search.isEmpty()) {
                                    Column(
                                        Modifier.fillMaxSize().padding(32.dp),
                                        horizontalAlignment = Alignment.CenterHorizontally,
                                        verticalArrangement = Arrangement.Center,
                                    ) {
                                        Surface(
                                            shape = RoundedCornerShape(32.dp),
                                            color = MaterialTheme.colorScheme.primaryContainer.copy(alpha = 0.5f),
                                            modifier = Modifier.size(100.dp),
                                        ) {
                                            Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                                                Icon(
                                                    Icons.Default.LibraryMusic,
                                                    null,
                                                    Modifier.size(52.dp),
                                                    tint = MaterialTheme.colorScheme.primary,
                                                )
                                            }
                                        }
                                        Spacer(Modifier.height(24.dp))
                                        Text(
                                            "Your music, in its own detail",
                                            style = MaterialTheme.typography.headlineSmall,
                                            fontWeight = FontWeight.Bold,
                                        )
                                        Spacer(Modifier.height(12.dp))
                                        Text(
                                            "Choose a music folder to build your library. Your files stay where they are.",
                                            style = MaterialTheme.typography.bodyMedium,
                                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                                        )
                                        Spacer(Modifier.height(24.dp))
                                        Button(
                                            onClick = chooseFolder,
                                            shape = RoundedCornerShape(20.dp),
                                        ) {
                                            Icon(Icons.Default.FolderOpen, null)
                                            Text("Choose music folder", Modifier.padding(start = 8.dp))
                                        }
                                    }
                                } else if (filter != "Songs") {
                                    val groups = visible.groupBy {
                                        if (filter == "Artists") it.artist to ""
                                        else it.album to it.albumArtist.ifBlank { it.artist }
                                    }
                                    LazyColumn(contentPadding = PaddingValues(bottom = 16.dp)) {
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
                                                            CircleShape,
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
                                        playing = snapshot?.transport == Transport.PLAYING,
                                        emptyText = "No songs match",
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
        AnimatedVisibility(
            visible = likedTrackForBanner != null,
            enter = slideInVertically(initialOffsetY = { it }) + fadeIn(),
            exit = slideOutVertically(targetOffsetY = { it }) + fadeOut(),
            modifier =
                Modifier.align(Alignment.BottomCenter)
                    .padding(
                        start = 16.dp,
                        end = 16.dp,
                        bottom =
                            if (nowPlaying) 24.dp
                            else if (snapshot?.current != null) 156.dp
                            else 92.dp,
                    ),
        ) {
            likedTrackForBanner?.let { track ->
                Surface(
                    shape = RoundedCornerShape(20.dp),
                    color = MaterialTheme.colorScheme.surfaceContainerHighest,
                    tonalElevation = 8.dp,
                    shadowElevation = 12.dp,
                    border =
                        BorderStroke(
                            1.dp,
                            MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.35f),
                        ),
                ) {
                    Row(
                        Modifier.padding(start = 16.dp, end = 8.dp, top = 8.dp, bottom = 8.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(12.dp),
                    ) {
                        Icon(
                            Icons.Default.Favorite,
                            null,
                            tint = MaterialTheme.colorScheme.primary,
                            modifier = Modifier.size(24.dp),
                        )
                        Column(Modifier.weight(1f, fill = false)) {
                            Text(
                                "Added to Favourites",
                                style = MaterialTheme.typography.labelLarge,
                                fontWeight = FontWeight.Bold,
                                color = MaterialTheme.colorScheme.onSurface,
                            )
                            Text(
                                track.title,
                                style = MaterialTheme.typography.bodySmall,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                        FilledTonalButton(
                            onClick = {
                                val toAdd = track
                                likedTrackForBanner = null
                                playlistPickerTrack = toAdd
                            },
                            shape = RoundedCornerShape(14.dp),
                            contentPadding =
                                PaddingValues(horizontal = 14.dp, vertical = 6.dp),
                        ) {
                            Text(
                                "Add to playlist",
                                style = MaterialTheme.typography.labelMedium,
                                fontWeight = FontWeight.SemiBold,
                            )
                        }
                        IconButton(
                            onClick = { likedTrackForBanner = null },
                            modifier = Modifier.size(32.dp),
                        ) {
                            Icon(
                                Icons.Default.Close,
                                "Dismiss",
                                Modifier.size(16.dp),
                                tint = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                    }
                }
            }
        }
    }
    if (filters) LibraryFilterDialog(library) { filters = false }
    if (sleepTimer) SleepTimerDialog(snapshot, library, dismiss = { sleepTimer = false })
    playlistPickerTrack?.let { track ->
        PlaylistPicker(library, track) { playlistPickerTrack = null }
    }
}
}
}

@Composable
fun Artwork(
    modifier: Modifier = Modifier,
    track: Track? = null,
    library: LibraryRepository? = null,
    shape: Shape = RoundedCornerShape(14.dp),
) {
    val generation = library?.features?.artworkGeneration?.collectAsState()?.value
    val bitmap by
        produceState<android.graphics.Bitmap?>(null, track?.id, track?.artRevision, generation) {
            value = if (track != null && library != null) library.features.artwork(track) else null
        }
    Surface(
        modifier = modifier,
        shape = shape,
        color = MaterialTheme.colorScheme.secondaryContainer,
        border = BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.25f)),
    ) {
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
    playing: Boolean = false,
    emptyText: String,
    play: (Track) -> Unit,
    favourite: (Track) -> Unit,
) {
    if (tracks.isEmpty()) {
        Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
            Text(
                emptyText,
                style = MaterialTheme.typography.bodyLarge,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    } else
        LazyColumn(
            contentPadding = PaddingValues(horizontal = 8.dp, vertical = 6.dp),
            verticalArrangement = Arrangement.spacedBy(4.dp),
        ) {
            itemsIndexed(tracks, key = { index, track -> "$index:${track.id}" }) { _, track ->
                val isSelected = current == track.id
                Surface(
                    shape = RoundedCornerShape(16.dp),
                    color =
                        if (isSelected) MaterialTheme.colorScheme.primaryContainer.copy(alpha = 0.22f)
                        else Color.Transparent,
                    border =
                        if (isSelected)
                            BorderStroke(
                                1.dp,
                                MaterialTheme.colorScheme.primary.copy(alpha = 0.35f),
                            )
                        else null,
                    modifier = Modifier.fillMaxWidth().animateItem(),
                ) {
                    Row(
                        Modifier.fillMaxWidth()
                            .clickable { play(track) }
                            .padding(horizontal = 12.dp, vertical = 8.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Artwork(Modifier.size(48.dp), track, library, RoundedCornerShape(12.dp))
                        Column(Modifier.weight(1f).padding(horizontal = 12.dp)) {
                            Row(
                                verticalAlignment = Alignment.CenterVertically,
                                horizontalArrangement = Arrangement.spacedBy(8.dp),
                            ) {
                                Text(
                                    track.title,
                                    style = MaterialTheme.typography.titleMedium,
                                    fontWeight = if (isSelected) FontWeight.Bold else FontWeight.Medium,
                                    maxLines = 1,
                                    overflow = TextOverflow.Ellipsis,
                                    color =
                                        if (isSelected) MaterialTheme.colorScheme.primary
                                        else MaterialTheme.colorScheme.onSurface,
                                    modifier = Modifier.weight(1f, fill = false),
                                )
                                if (isSelected)
                                    PlayingEqualizerBars(
                                        playing = playing,
                                        modifier = Modifier.size(14.dp),
                                        color = MaterialTheme.colorScheme.primary,
                                    )
                            }
                            Row(
                                verticalAlignment = Alignment.CenterVertically,
                                horizontalArrangement = Arrangement.spacedBy(6.dp),
                            ) {
                                Text(
                                    track.artist.ifBlank { "Unknown artist" },
                                    style = MaterialTheme.typography.bodyMedium,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    maxLines = 1,
                                    overflow = TextOverflow.Ellipsis,
                                    modifier = Modifier.weight(1f, fill = false),
                                )
                                AudioFormatBadge(track)
                            }
                        }
                        IconButton(onClick = { inspect(track) }) {
                            Icon(
                                Icons.Default.Info,
                                "Inspect ${track.title}",
                                Modifier.size(20.dp),
                                tint = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                        FavouriteButton(track, library)
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
    val playing =
        snapshot.transport == Transport.PLAYING || snapshot.transport == Transport.LOADING
    Surface(
        shape = RoundedCornerShape(28.dp),
        border =
            BorderStroke(
                1.dp,
                Brush.horizontalGradient(
                    listOf(
                        tones.primary.copy(alpha = 0.55f),
                        tones.secondary.copy(alpha = 0.40f),
                    )
                ),
            ),
        color = MaterialTheme.colorScheme.surfaceContainerHighest.copy(alpha = 0.88f),
        tonalElevation = 6.dp,
        shadowElevation = 12.dp,
        modifier =
            Modifier.fillMaxWidth()
                .padding(start = 14.dp, end = 14.dp, top = 2.dp, bottom = 8.dp),
    ) {
        AmbientBackdrop(
            tones,
            strength = 0.45f,
            base = MaterialTheme.colorScheme.surfaceContainerHighest.copy(alpha = 0.70f),
        ) {
            Row(
                Modifier.fillMaxWidth()
                    .clickable(onClickLabel = "Open now playing", onClick = open)
                    .padding(start = 10.dp, end = 8.dp, top = 8.dp, bottom = 8.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Artwork(Modifier.size(46.dp), snapshot.current, library, RoundedCornerShape(12.dp))
                Crossfade(
                    targetState =
                        PlayerLabel(
                            snapshot.current?.id,
                            snapshot.current?.title.orEmpty(),
                            snapshot.current?.artist.orEmpty(),
                        ),
                    animationSpec = tween(220),
                    modifier = Modifier.weight(1f).padding(start = 10.dp, end = 4.dp),
                    label = "Mini player song handover",
                ) { label ->
                    Column {
                        Row(
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(6.dp),
                        ) {
                            Text(
                                label.title,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                                style = MaterialTheme.typography.titleMedium,
                                fontWeight = FontWeight.SemiBold,
                                modifier = Modifier.weight(1f, fill = false),
                            )
                            PlayingEqualizerBars(
                                playing = playing,
                                modifier = Modifier.size(13.dp),
                                color = tones.primary,
                            )
                        }
                        Text(
                            label.artist.ifBlank { "Unknown artist" },
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
                Box(Modifier.size(0.dp).semantics { contentDescription = "Song progress" })
                IconButton(
                    onClick = { controller?.seekToPreviousMediaItem() },
                    enabled = controller != null,
                    modifier = Modifier.size(38.dp),
                ) {
                    Icon(
                        Icons.Default.SkipPrevious,
                        "Previous song",
                        Modifier.size(20.dp),
                    )
                }
                PlayButton(snapshot, controller)
                IconButton(
                    onClick = { controller?.seekToNextMediaItem() },
                    enabled = controller != null,
                    modifier = Modifier.size(38.dp),
                ) {
                    Icon(
                        Icons.Default.SkipNext,
                        "Next song",
                        Modifier.size(20.dp),
                    )
                }
                FavouriteButton(
                    snapshot.current,
                    library,
                    current = true,
                    modifier = Modifier.size(38.dp),
                )
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
    val buttonScale =
        animateFloatAsState(
            if (playing) 1.05f else 1f,
            spring(
                dampingRatio = Spring.DampingRatioMediumBouncy,
                stiffness = Spring.StiffnessMedium,
            ),
            label = "Play button bounce",
        )
    FilledIconButton(
        onClick = { if (playing) controller?.pause() else controller?.play() },
        enabled = controller != null,
        shape = if (expanded) RoundedCornerShape(24.dp) else CircleShape,
        colors =
            IconButtonDefaults.filledIconButtonColors(
                containerColor = MaterialTheme.colorScheme.primary,
                contentColor = MaterialTheme.colorScheme.onPrimary,
            ),
        modifier =
            Modifier.size(if (expanded) 72.dp else 42.dp)
                .graphicsLayer {
                    scaleX = buttonScale.value
                    scaleY = buttonScale.value
                }
                .semantics {
                    contentDescription = if (playing) "Pause" else "Play"
                },
    ) {
        Crossfade(playing, animationSpec = tween(160), label = "Play pause icon") { active ->
            Icon(
                if (active) Icons.Default.Pause else Icons.Default.PlayArrow,
                null,
                Modifier.size(if (expanded) 36.dp else 22.dp),
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
    AmbientBackdrop(tones, Modifier.fillMaxSize(), strength = 0.55f, animated = true) {
        Column(
            Modifier.fillMaxWidth()
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 24.dp, vertical = 20.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            BoxWithConstraints(Modifier.fillMaxWidth(), contentAlignment = Alignment.Center) {
                val edge = minOf(maxWidth - 24.dp, 280.dp)
                ArtworkSpotlight(
                    Modifier.size(edge),
                    playing = playing,
                    glowColor = tones.primary,
                    shape = RoundedCornerShape(28.dp),
                ) {
                    Artwork(Modifier.fillMaxSize(), track, library, RoundedCornerShape(28.dp))
                }
            }
            Spacer(Modifier.height(24.dp))
            Row(
                Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.SpaceBetween,
            ) {
                Column(Modifier.weight(1f).padding(end = 8.dp)) {
                    Text(
                        track?.title ?: "Nothing playing",
                        style = MaterialTheme.typography.headlineSmall,
                        fontWeight = FontWeight.Bold,
                        maxLines = 2,
                        overflow = TextOverflow.Ellipsis,
                    )
                    track?.let {
                        Spacer(Modifier.height(4.dp))
                        AudioFormatBadge(it)
                    }
                }
                FavouriteButton(track, library, current = true)
            }
            Spacer(Modifier.height(12.dp))
            Row(
                Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(10.dp),
            ) {
                Surface(
                    shape = RoundedCornerShape(16.dp),
                    color = MaterialTheme.colorScheme.surfaceContainerHighest.copy(alpha = 0.6f),
                    border =
                        BorderStroke(
                            1.dp,
                            MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.3f),
                        ),
                    modifier =
                        Modifier.weight(1f)
                            .clickable(enabled = !track?.artist.isNullOrBlank()) {
                                track?.let { openArtist(it.artist) }
                            }
                            .semantics {
                                contentDescription = "Open artist ${track?.artist.orEmpty()}"
                            },
                ) {
                    Row(
                        Modifier.padding(horizontal = 12.dp, vertical = 8.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.Center,
                    ) {
                        Icon(
                            Icons.Default.Person,
                            null,
                            Modifier.size(16.dp),
                            tint = MaterialTheme.colorScheme.primary,
                        )
                        Text(
                            track?.artist?.ifBlank { "Unknown artist" }.orEmpty(),
                            Modifier.padding(start = 6.dp),
                            style = MaterialTheme.typography.bodyMedium,
                            fontWeight = FontWeight.Medium,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                    }
                }
                Surface(
                    shape = RoundedCornerShape(16.dp),
                    color = MaterialTheme.colorScheme.surfaceContainerHighest.copy(alpha = 0.6f),
                    border =
                        BorderStroke(
                            1.dp,
                            MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.3f),
                        ),
                    modifier =
                        Modifier.weight(1f)
                            .clickable(enabled = !track?.album.isNullOrBlank()) {
                                track?.let(openAlbum)
                            }
                            .semantics {
                                contentDescription = "Open album ${track?.album.orEmpty()}"
                            },
                ) {
                    Row(
                        Modifier.padding(horizontal = 12.dp, vertical = 8.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.Center,
                    ) {
                        Icon(
                            Icons.Default.Album,
                            null,
                            Modifier.size(16.dp),
                            tint = MaterialTheme.colorScheme.primary,
                        )
                        Text(
                            track?.album.orEmpty().ifBlank { "No album" },
                            Modifier.padding(start = 6.dp),
                            style = MaterialTheme.typography.bodyMedium,
                            fontWeight = FontWeight.Medium,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                    }
                }
            }
            Spacer(Modifier.height(16.dp))
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
                colors =
                    SliderDefaults.colors(
                        thumbColor = MaterialTheme.colorScheme.primary,
                        activeTrackColor = MaterialTheme.colorScheme.primary,
                        inactiveTrackColor = MaterialTheme.colorScheme.surfaceContainerHighest,
                    ),
            )
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
                Text(
                    clock(snapshot?.positionMs ?: 0uL),
                    style = MaterialTheme.typography.labelMedium,
                    fontWeight = FontWeight.Medium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Text(
                    clock(snapshot?.durationMs ?: 0uL),
                    style = MaterialTheme.typography.labelMedium,
                    fontWeight = FontWeight.Medium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            Spacer(Modifier.height(8.dp))
            Row(
                Modifier.fillMaxWidth().padding(vertical = 12.dp),
                horizontalArrangement = Arrangement.SpaceEvenly,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                FilledTonalIconButton(
                    onClick = { controller?.shuffleModeEnabled = !(snapshot?.shuffle ?: false) },
                    shape = CircleShape,
                    colors =
                        IconButtonDefaults.filledTonalIconButtonColors(
                            containerColor =
                                if (snapshot?.shuffle == true)
                                    MaterialTheme.colorScheme.primaryContainer
                                else MaterialTheme.colorScheme.surfaceContainerHigh,
                            contentColor =
                                if (snapshot?.shuffle == true)
                                    MaterialTheme.colorScheme.onPrimaryContainer
                                else MaterialTheme.colorScheme.onSurfaceVariant,
                        ),
                ) {
                    Icon(
                        Icons.Default.Shuffle,
                        "Shuffle",
                    )
                }
                FilledTonalIconButton(
                    onClick = { controller?.seekToPreviousMediaItem() },
                    shape = CircleShape,
                ) {
                    Icon(Icons.Default.SkipPrevious, "Previous song")
                }
                PlayButton(snapshot, controller, expanded = true)
                FilledTonalIconButton(
                    onClick = { controller?.seekToNextMediaItem() },
                    shape = CircleShape,
                ) {
                    Icon(Icons.Default.SkipNext, "Next song")
                }
                FilledTonalIconButton(
                    onClick = {
                        controller?.repeatMode = ((snapshot?.repeat?.toInt() ?: 0) + 1) % 3
                    },
                    shape = CircleShape,
                    colors =
                        IconButtonDefaults.filledTonalIconButtonColors(
                            containerColor =
                                if (snapshot?.repeat != 0u)
                                    MaterialTheme.colorScheme.primaryContainer
                                else MaterialTheme.colorScheme.surfaceContainerHigh,
                            contentColor =
                                if (snapshot?.repeat != 0u)
                                    MaterialTheme.colorScheme.onPrimaryContainer
                                else MaterialTheme.colorScheme.onSurfaceVariant,
                        ),
                ) {
                    Icon(
                        if (snapshot?.repeat == 1u) Icons.Default.RepeatOne
                        else Icons.Default.Repeat,
                        "Repeat",
                    )
                }
            }
            Spacer(Modifier.height(8.dp))
            Row(
                Modifier.horizontalScroll(rememberScrollState()).padding(vertical = 4.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                FilledTonalButton(
                    onClick = openSound,
                    shape = RoundedCornerShape(16.dp),
                    contentPadding = PaddingValues(horizontal = 14.dp, vertical = 6.dp),
                ) {
                    Icon(Icons.Default.Equalizer, null, Modifier.size(18.dp))
                    Text("Equaliser", Modifier.padding(start = 6.dp))
                }
                FilledTonalButton(
                    onClick = openSleep,
                    shape = RoundedCornerShape(16.dp),
                    contentPadding = PaddingValues(horizontal = 14.dp, vertical = 6.dp),
                ) {
                    Icon(Icons.Default.Bedtime, null, Modifier.size(18.dp))
                    Text("Sleep timer", Modifier.padding(start = 6.dp))
                }
                listOf(
                        "Lyrics" to Icons.Default.Lyrics,
                        "Visualizer" to Icons.Default.GraphicEq,
                        "Analysis" to Icons.Default.Insights,
                        "Inspector" to Icons.Default.Info,
                    )
                    .forEach { (destination, icon) ->
                        FilledTonalButton(
                            onClick = { openTools(destination) },
                            shape = RoundedCornerShape(16.dp),
                            contentPadding = PaddingValues(horizontal = 14.dp, vertical = 6.dp),
                        ) {
                            Icon(icon, null, Modifier.size(18.dp))
                            Text(destination, Modifier.padding(start = 6.dp))
                        }
                    }
            }
            if (snapshot?.sleep != null) {
                Spacer(Modifier.height(8.dp))
                Surface(
                    shape = RoundedCornerShape(12.dp),
                    color = MaterialTheme.colorScheme.primaryContainer.copy(alpha = 0.4f),
                    border = BorderStroke(1.dp, MaterialTheme.colorScheme.primary.copy(alpha = 0.2f)),
                ) {
                    Text(
                        sleepDescription(snapshot),
                        modifier = Modifier.padding(horizontal = 12.dp, vertical = 6.dp),
                        style = MaterialTheme.typography.bodySmall,
                        fontWeight = FontWeight.Medium,
                        color = MaterialTheme.colorScheme.primary,
                    )
                }
            }
            track?.let {
                Spacer(Modifier.height(12.dp))
                val sourceRate = (it.rate.toDouble() / 1000).toString().removeSuffix(".0")
                val outputRate =
                    snapshot.outputRate
                        .takeIf { rate -> rate > 0u }
                        ?.let { rate ->
                            "${(rate.toDouble() / 1000).toString().removeSuffix(".0")} kHz output"
                        }
                Surface(
                    shape = RoundedCornerShape(12.dp),
                    color = MaterialTheme.colorScheme.surfaceContainerHigh.copy(alpha = 0.5f),
                    border = BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.2f)),
                ) {
                    Text(
                        listOfNotNull("${it.codec} · $sourceRate kHz source", outputRate)
                            .joinToString(" · "),
                        modifier = Modifier.padding(horizontal = 12.dp, vertical = 6.dp),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        }
    }
}

private fun clock(milliseconds: ULong): String {
    val seconds = milliseconds / 1000u
    return "${seconds / 60u}:${(seconds % 60u).toString().padStart(2, '0')}"
}
