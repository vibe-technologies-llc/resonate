package org.resonate.android

import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.Spring
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.spring
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
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
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Album
import androidx.compose.material.icons.filled.Analytics
import androidx.compose.material.icons.filled.Audiotrack
import androidx.compose.material.icons.filled.AutoAwesome
import androidx.compose.material.icons.filled.GraphicEq
import androidx.compose.material.icons.filled.Info
import androidx.compose.material.icons.filled.LibraryMusic
import androidx.compose.material.icons.filled.Lyrics
import androidx.compose.material.icons.filled.MusicNote
import androidx.compose.material.icons.filled.Pause
import androidx.compose.material.icons.filled.Person
import androidx.compose.material.icons.automirrored.filled.PlaylistAdd
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.SkipNext
import androidx.compose.material.icons.filled.SkipPrevious
import androidx.compose.material.icons.filled.Speed
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.FilterChip
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.blur
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import androidx.media3.session.MediaController
import java.util.Locale
import kotlin.math.abs
import org.resonate.android.nativecore.Snapshot
import org.resonate.android.nativecore.Track
import org.resonate.android.nativecore.TrackOrder
import org.resonate.android.nativecore.defaultTrackFilter

@Composable
fun FolderScreen(library: LibraryRepository, chooseFolder: () -> Unit) {
    val folders by library.features.folders.collectAsState()
    var removing by remember { mutableStateOf<MusicFolder?>(null) }
    LaunchedEffect(Unit) { library.features.refreshFolders() }
    LazyColumn(
        Modifier.fillMaxSize().padding(horizontal = 20.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        item {
            Text("Music folders", style = MaterialTheme.typography.headlineSmall)
            Text(
                "Music is read through the folders you grant.",
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        item { Button(onClick = chooseFolder, shape = RoundedCornerShape(16.dp)) { Text("Add music folder") } }
        if (folders.isEmpty()) item { Text("No music folders have been added.") }
        items(folders, key = { it.uri.toString() }) { folder ->
            Surface(
                shape = RoundedCornerShape(20.dp),
                color = MaterialTheme.colorScheme.surfaceContainerLow,
                border = BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.25f)),
                modifier = Modifier.fillMaxWidth(),
            ) {
                Column(Modifier.padding(18.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                    Text(folder.name, style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
                    Text(
                        if (folder.readable) "Access available"
                        else "Unavailable · choose this folder again to restore access",
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        style = MaterialTheme.typography.bodyMedium,
                    )
                    Text(folder.uri.toString(), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        TextButton(
                            onClick = { library.scan(folder.uri) },
                            enabled = folder.readable,
                        ) {
                            Text("Rescan")
                        }
                        TextButton(onClick = { removing = folder }) { Text("Remove access") }
                    }
                }
            }
        }
    }
    removing?.let { folder ->
        AlertDialog(
            onDismissRequest = { removing = null },
            shape = RoundedCornerShape(28.dp),
            title = { Text("Remove ${folder.name}?") },
            text = {
                Text(
                    "Your music files stay where they are. Cached library entries remain, but playback from this folder will need access again."
                )
            },
            confirmButton = {
                TextButton(
                    onClick = {
                        library.features.removeFolder(folder)
                        removing = null
                    }
                ) {
                    Text("Remove access")
                }
            },
            dismissButton = { TextButton(onClick = { removing = null }) { Text("Cancel") } },
        )
    }
}

@Composable
fun StatisticsScreen(library: LibraryRepository, play: (List<Track>, Track) -> Unit) {
    val statistics by library.features.statistics.collectAsState()
    var days by remember { mutableStateOf(0u) }
    LaunchedEffect(days) { library.features.loadStatistics(days) }
    LazyColumn(
        Modifier.fillMaxSize().padding(horizontal = 20.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        item {
            Text(
                "Listening statistics",
                style = MaterialTheme.typography.headlineSmall,
                fontWeight = FontWeight.Bold,
            )
        }
        item {
            Row(
                Modifier.horizontalScroll(rememberScrollState()),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                listOf(7u to "Week", 30u to "Month", 365u to "Year", 0u to "All time").forEach {
                    (value, label) ->
                    FilterChip(
                        days == value,
                        { days = value },
                        label = { Text(label) },
                        shape = RoundedCornerShape(14.dp),
                    )
                }
            }
        }
        statistics?.let { value ->
            item {
                Surface(
                    shape = RoundedCornerShape(24.dp),
                    color = MaterialTheme.colorScheme.surfaceContainerLow,
                    border = BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.25f)),
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Column(Modifier.padding(20.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                        Text(
                            "${value.plays} plays · ${value.listenedMs / 60000uL} minutes listened",
                            style = MaterialTheme.typography.titleLarge,
                            fontWeight = FontWeight.Bold,
                        )
                        Text(
                            "${value.songs} songs · ${value.albums} albums · ${value.artists} artists",
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            style = MaterialTheme.typography.bodyMedium,
                        )
                        Text(
                            "${value.durationMs / 3600000uL} hours in your library",
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            style = MaterialTheme.typography.bodyMedium,
                        )
                    }
                }
            }
            item {
                Text(
                    "Most listened",
                    style = MaterialTheme.typography.titleLarge,
                    fontWeight = FontWeight.Bold,
                )
            }
            if (value.topTracks.isEmpty())
                item { Text("Play a song for half its length, or four minutes, to count a play.") }
            items(value.topTracks, key = { it.id.toString() }) { track ->
                TrackCard(track, library) { play(value.topTracks, track) }
            }
        }
    }
}

@Composable
fun SuggestionsScreen(library: LibraryRepository, openLibrary: () -> Unit) {
    val statistics by library.features.statistics.collectAsState()
    LaunchedEffect(Unit) { library.features.loadStatistics() }
    LazyColumn(
        Modifier.fillMaxSize().padding(horizontal = 20.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        item {
            Text(
                "Suggestions",
                style = MaterialTheme.typography.headlineSmall,
                fontWeight = FontWeight.Bold,
            )
            Text(
                "Mixes from your library and listening history",
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        listOf(
                "Never heard" to defaultTrackFilter().copy(unplayed = true),
                "Your favourites" to defaultTrackFilter().copy(favourites = true),
                "Most played" to
                    defaultTrackFilter().copy(order = TrackOrder.PLAYS, descending = true),
                "Recently added" to
                    defaultTrackFilter().copy(order = TrackOrder.ADDED, descending = true),
                "High-resolution music" to defaultTrackFilter().copy(minimumRate = 88200u),
            )
            .forEach { (label, filter) ->
                item {
                    Surface(
                        shape = RoundedCornerShape(20.dp),
                        color = MaterialTheme.colorScheme.surfaceContainerLow,
                        border = BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.25f)),
                        modifier =
                            Modifier.fillMaxWidth().clickable {
                                library.setFilter(filter)
                                openLibrary()
                            },
                    ) {
                        Column(Modifier.padding(20.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                            Text(label, style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.Bold)
                            Text("Explore this mix", color = MaterialTheme.colorScheme.primary, style = MaterialTheme.typography.labelLarge)
                        }
                    }
                }
            }
        statistics
            ?.topTracks
            ?.firstOrNull()
            ?.artist
            ?.takeIf { it.isNotBlank() }
            ?.let { artist ->
                item {
                    Card(
                        Modifier.fillMaxWidth().clickable {
                            library.setFilter(defaultTrackFilter().copy(artist = artist))
                            openLibrary()
                        }
                    ) {
                        Text(
                            "More from $artist",
                            Modifier.padding(20.dp),
                            style = MaterialTheme.typography.titleLarge,
                        )
                    }
                }
            }
    }
}

@Composable
fun LibraryFilterDialog(library: LibraryRepository, dismiss: () -> Unit) {
    val current by library.filter.collectAsState()
    var filter by remember { mutableStateOf(current ?: defaultTrackFilter()) }
    AlertDialog(
        onDismissRequest = dismiss,
        title = { Text("Filter and sort") },
        text = {
            Column(
                Modifier.verticalScroll(rememberScrollState()),
                verticalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                OutlinedTextField(
                    filter.genre,
                    { filter = filter.copy(genre = it) },
                    label = { Text("Genre") },
                    singleLine = true,
                )
                OutlinedTextField(
                    filter.artist,
                    { filter = filter.copy(artist = it) },
                    label = { Text("Artist") },
                    singleLine = true,
                )
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text("Favourites only", Modifier.weight(1f))
                    Switch(filter.favourites, { filter = filter.copy(favourites = it) })
                }
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text("Unplayed only", Modifier.weight(1f))
                    Switch(filter.unplayed, { filter = filter.copy(unplayed = it) })
                }
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text("High-resolution only", Modifier.weight(1f))
                    Switch(
                        filter.minimumRate > 48000u,
                        { filter = filter.copy(minimumRate = if (it) 88200u else 0u) },
                    )
                }
                Text("Sort by", style = MaterialTheme.typography.titleSmall)
                FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    TrackOrder.entries.forEach { order ->
                        FilterChip(
                            filter.order == order,
                            { filter = filter.copy(order = order) },
                            label = {
                                Text(order.name.lowercase().replaceFirstChar { it.uppercase() })
                            },
                        )
                    }
                }
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text("Descending", Modifier.weight(1f))
                    Switch(filter.descending, { filter = filter.copy(descending = it) })
                }
            }
        },
        confirmButton = {
            TextButton(
                onClick = {
                    library.setFilter(filter)
                    dismiss()
                }
            ) {
                Text("Apply")
            }
        },
        dismissButton = {
            TextButton(
                onClick = {
                    library.setFilter(defaultTrackFilter())
                    dismiss()
                }
            ) {
                Text("Reset")
            }
        },
    )
}

@Composable
fun TrackCard(track: Track, library: LibraryRepository, click: () -> Unit) {
    Surface(
        shape = RoundedCornerShape(18.dp),
        color = MaterialTheme.colorScheme.surfaceContainerLow,
        border = BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.2f)),
        modifier = Modifier.fillMaxWidth().clickable(onClick = click),
    ) {
        Row(
            Modifier.fillMaxWidth().padding(horizontal = 14.dp, vertical = 10.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Artwork(Modifier.size(52.dp), track, library, RoundedCornerShape(12.dp))
            Column(Modifier.weight(1f).padding(start = 12.dp)) {
                Text(
                    track.title,
                    style = MaterialTheme.typography.titleMedium,
                    fontWeight = FontWeight.SemiBold,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                Text(
                    track.artist.ifBlank { "Unknown artist" },
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    style = MaterialTheme.typography.bodyMedium,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
    }
}

@Composable
fun InspectorScreen(library: LibraryRepository, track: Track?) {
    var choosingPlaylist by remember { mutableStateOf(false) }
    val inspector by library.features.inspector.collectAsState()
    val artist by library.features.artist.collectAsState()
    val online by library.features.online.collectAsState()
    val tones by rememberArtworkTones(track, library)
    LaunchedEffect(track?.id) { track?.let { library.features.loadInspector(it.id) } }
    if (track == null) {
        Text("Choose a song to inspect", Modifier.padding(20.dp))
        return
    }
    AmbientBackdrop(tones, Modifier.fillMaxSize(), strength = 0.40f, animated = true) {
        LazyColumn(
            Modifier.fillMaxSize().padding(horizontal = 20.dp),
            contentPadding = PaddingValues(vertical = 20.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            item {
                Surface(
                    shape = RoundedCornerShape(28.dp),
                    color = MaterialTheme.colorScheme.surfaceContainerLow.copy(alpha = 0.75f),
                    border = BorderStroke(1.dp, tones.primary.copy(alpha = 0.35f)),
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Column(
                        Modifier.padding(20.dp),
                        horizontalAlignment = Alignment.CenterHorizontally,
                    ) {
                        ArtworkSpotlight(
                            Modifier.size(160.dp),
                            playing = false,
                            glowColor = tones.primary,
                            shape = RoundedCornerShape(22.dp),
                        ) {
                            Artwork(Modifier.fillMaxSize(), track, library, RoundedCornerShape(22.dp))
                        }
                        Spacer(Modifier.height(16.dp))
                        Text(
                            track.title,
                            style = MaterialTheme.typography.headlineSmall,
                            fontWeight = FontWeight.Bold,
                        )
                        Text(
                            track.artist,
                            style = MaterialTheme.typography.titleMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                        Spacer(Modifier.height(8.dp))
                        AudioFormatBadge(track)
                        Spacer(Modifier.height(14.dp))
                        Row(
                            horizontalArrangement = Arrangement.spacedBy(8.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            FilledTonalButton(
                                onClick = { choosingPlaylist = true },
                                shape = RoundedCornerShape(16.dp),
                            ) {
                                Icon(Icons.AutoMirrored.Filled.PlaylistAdd, null, Modifier.size(18.dp))
                                Spacer(Modifier.size(6.dp))
                                Text("Add to playlist")
                            }
                        }
                    }
                }
            }
            item {
                Surface(
                    shape = RoundedCornerShape(22.dp),
                    color = MaterialTheme.colorScheme.surfaceContainerLow.copy(alpha = 0.75f),
                    border =
                        BorderStroke(
                            1.dp,
                            MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.35f),
                        ),
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Column(Modifier.padding(20.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                        Row(
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(8.dp),
                        ) {
                            Icon(
                                Icons.Default.Audiotrack,
                                null,
                                Modifier.size(20.dp),
                                tint = tones.primary,
                            )
                            Text(
                                "Source audio",
                                style = MaterialTheme.typography.titleLarge,
                                fontWeight = FontWeight.Bold,
                            )
                        }
                        Text(
                            "${track.codec} · ${track.rate} Hz · ${track.channels} channels · ${track.bits ?: "Unknown"} bits",
                            style = MaterialTheme.typography.bodyMedium,
                        )
                        Text(
                            "Duration: ${timeLabel(track.durationMs)}",
                            style = MaterialTheme.typography.bodyMedium,
                        )
                        Text(
                            track.uri,
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
            }
            item {
                Surface(
                    shape = RoundedCornerShape(22.dp),
                    color = MaterialTheme.colorScheme.surfaceContainerLow.copy(alpha = 0.75f),
                    border =
                        BorderStroke(
                            1.dp,
                            MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.35f),
                        ),
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Column(Modifier.padding(20.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                        Row(
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(8.dp),
                        ) {
                            Icon(
                                Icons.Default.Album,
                                null,
                                Modifier.size(20.dp),
                                tint = tones.primary,
                            )
                            Text(
                                "Metadata & Tags",
                                style = MaterialTheme.typography.titleLarge,
                                fontWeight = FontWeight.Bold,
                            )
                        }
                        Text("Album: ${track.album}")
                        Text("Album artist: ${track.albumArtist.ifBlank { "Not tagged" }}")
                        Text("Genre: ${track.genre.ifBlank { "Not tagged" }}")
                        Text("Date: ${track.date.ifBlank { "Not tagged" }}")
                        Text("Disc ${track.discNumber ?: "—"} · Track ${track.trackNumber ?: "—"}")
                    }
                }
            }
            inspector
                ?.takeIf { it.track.id == track.id }
                ?.let { value ->
                    item {
                        Surface(
                            shape = RoundedCornerShape(22.dp),
                            color = MaterialTheme.colorScheme.surfaceContainerLow.copy(alpha = 0.75f),
                            border =
                                BorderStroke(
                                    1.dp,
                                    MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.35f),
                                ),
                            modifier = Modifier.fillMaxWidth(),
                        ) {
                            Column(Modifier.padding(20.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                                Row(
                                    verticalAlignment = Alignment.CenterVertically,
                                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                                ) {
                                    Icon(
                                        Icons.Default.Info,
                                        null,
                                        Modifier.size(20.dp),
                                        tint = tones.primary,
                                    )
                                    Text(
                                        "Technical container",
                                        style = MaterialTheme.typography.titleLarge,
                                        fontWeight = FontWeight.Bold,
                                    )
                                }
                                Text("Container: ${value.container}")
                                Text("Decoded format: ${value.sampleFormat}")
                                Text("Seekable: ${if (value.seekable) "Yes" else "No"}")
                                Text("Encoder: ${value.encoder.ifBlank { "Not tagged" }}")
                                Text("ISRC: ${value.isrc.ifBlank { "Not tagged" }}")
                                Text("Recording ID: ${value.track.recording ?: "Not identified"}")
                                Text("Release ID: ${value.track.release ?: "Not identified"}")
                                if (value.copyright.isNotBlank()) Text(value.copyright)
                                if (value.comment.isNotBlank()) Text(value.comment)
                            }
                        }
                    }
                }
            if (online)
                item {
                    Surface(
                        shape = RoundedCornerShape(22.dp),
                        color = MaterialTheme.colorScheme.surfaceContainerLow.copy(alpha = 0.75f),
                        border =
                            BorderStroke(
                                1.dp,
                                MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.35f),
                            ),
                        modifier = Modifier.fillMaxWidth(),
                    ) {
                        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                            TextButton(onClick = { library.features.enrich(listOf(track)) }) {
                                Icon(Icons.Default.AutoAwesome, null, Modifier.size(18.dp))
                                Spacer(Modifier.size(8.dp))
                                Text("Enrich metadata, artwork and lyrics")
                            }
                            TextButton(onClick = { library.features.loadArtist(track.id) }) {
                                Icon(Icons.Default.Person, null, Modifier.size(18.dp))
                                Spacer(Modifier.size(8.dp))
                                Text("Look up artist")
                            }
                        }
                    }
                }
            artist?.let { value ->
                item {
                    HorizontalDivider()
                    ArtistInformationCard(value)
                }
            }
        }
    }
    if (choosingPlaylist) PlaylistPicker(library, track) { choosingPlaylist = false }
}

@Composable
fun AnalysisScreen(library: LibraryRepository, track: Track?) {
    val report by library.features.analysis.collectAsState()
    val busy by library.features.analysisBusy.collectAsState()
    val progress by library.features.analysisProgress.collectAsState()
    val online by library.features.online.collectAsState()
    val candidates by library.features.recognition.collectAsState()
    val recognising by library.features.recognitionBusy.collectAsState()
    val savedKey by library.features.recognitionKey.collectAsState()
    var key by remember(savedKey) { mutableStateOf(savedKey) }
    val uriHandler = androidx.compose.ui.platform.LocalUriHandler.current
    val tones by rememberArtworkTones(track, library)
    LaunchedEffect(track?.id) { track?.let { library.features.analyse(it.id) } }
    DisposableEffect(Unit) { onDispose { library.features.cancelAnalysis() } }
    AmbientBackdrop(tones, Modifier.fillMaxSize(), strength = 0.45f, animated = true) {
        Column(
            Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(20.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            Surface(
                shape = RoundedCornerShape(24.dp),
                color = MaterialTheme.colorScheme.surfaceContainerLow.copy(alpha = 0.75f),
                border = BorderStroke(1.dp, tones.primary.copy(alpha = 0.35f)),
                modifier = Modifier.fillMaxWidth(),
            ) {
                Row(
                    Modifier.padding(16.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(14.dp),
                ) {
                    Artwork(Modifier.size(56.dp), track, library, RoundedCornerShape(14.dp))
                    Column(Modifier.weight(1f)) {
                        Text(
                            track?.title ?: "Choose a song to analyse",
                            style = MaterialTheme.typography.titleLarge,
                            fontWeight = FontWeight.Bold,
                        )
                        Text(
                            track?.artist.orEmpty().ifBlank { "Audio mastering analysis" },
                            style = MaterialTheme.typography.bodyMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                        Spacer(Modifier.height(4.dp))
                        AudioFormatBadge(track)
                    }
                }
            }
            if (busy) {
                Surface(
                    shape = RoundedCornerShape(20.dp),
                    color = MaterialTheme.colorScheme.surfaceContainerHigh.copy(alpha = 0.8f),
                    border = BorderStroke(1.dp, tones.primary.copy(alpha = 0.4f)),
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                        Row(
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(8.dp),
                        ) {
                            CircularProgressIndicator(
                                modifier = Modifier.size(20.dp),
                                strokeWidth = 2.5.dp,
                                color = tones.primary,
                            )
                            Text(
                                "Analysing audio…",
                                style = MaterialTheme.typography.titleMedium,
                                fontWeight = FontWeight.SemiBold,
                            )
                        }
                        progress?.let {
                            LinearProgressIndicator(
                                progress = { it },
                                modifier = Modifier.fillMaxWidth(),
                                strokeCap = StrokeCap.Round,
                                color = tones.primary,
                            )
                        } ?: LinearProgressIndicator(
                            Modifier.fillMaxWidth(),
                            strokeCap = StrokeCap.Round,
                            color = tones.primary,
                        )
                        TextButton(onClick = library.features::cancelAnalysis) {
                            Text("Cancel analysis")
                        }
                    }
                }
            }
            report
                ?.takeIf { it.trackId == track?.id }
                ?.let { value ->
                    Surface(
                        shape = RoundedCornerShape(22.dp),
                        color = MaterialTheme.colorScheme.surfaceContainerLow.copy(alpha = 0.75f),
                        border =
                            BorderStroke(
                                1.dp,
                                MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.35f),
                            ),
                        modifier = Modifier.fillMaxWidth(),
                    ) {
                        Column(Modifier.padding(18.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                            Row(
                                verticalAlignment = Alignment.CenterVertically,
                                horizontalArrangement = Arrangement.spacedBy(8.dp),
                            ) {
                                Icon(
                                    Icons.Default.Analytics,
                                    null,
                                    Modifier.size(22.dp),
                                    tint = tones.primary,
                                )
                                Text(
                                    "Assessment: ${value.verdict}",
                                    style = MaterialTheme.typography.titleLarge,
                                    fontWeight = FontWeight.Bold,
                                )
                            }
                            Text(
                                "Integrated loudness: ${value.loudnessLufs?.let { String.format(Locale.getDefault(), "%.1f LUFS", it) } ?: "Not measurable"}",
                                style = MaterialTheme.typography.bodyMedium,
                            )
                            Text(
                                "True peak: ${String.format(Locale.getDefault(), "%.5f", value.truePeak)} · Dynamic range: ${value.dynamicRange ?: "—"}",
                                style = MaterialTheme.typography.bodyMedium,
                            )
                            Text(
                                "Clipped samples: ${value.clipped} · Bits in use: ${value.bitsInUse ?: "—"}",
                                style = MaterialTheme.typography.bodyMedium,
                            )
                            if (value.findings.isNotEmpty()) {
                                Spacer(Modifier.height(6.dp))
                                value.findings.forEach {
                                    Text(
                                        it,
                                        style = MaterialTheme.typography.bodySmall,
                                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    )
                                }
                            }
                        }
                    }
                    Surface(
                        shape = RoundedCornerShape(22.dp),
                        color = MaterialTheme.colorScheme.surfaceContainerLow.copy(alpha = 0.75f),
                        border =
                            BorderStroke(
                                1.dp,
                                MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.35f),
                            ),
                        modifier = Modifier.fillMaxWidth(),
                    ) {
                        Column(Modifier.padding(18.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                            Row(
                                verticalAlignment = Alignment.CenterVertically,
                                horizontalArrangement = Arrangement.spacedBy(8.dp),
                            ) {
                                Icon(
                                    Icons.Default.GraphicEq,
                                    null,
                                    Modifier.size(20.dp),
                                    tint = tones.primary,
                                )
                                Text(
                                    "Waveform",
                                    style = MaterialTheme.typography.titleLarge,
                                    fontWeight = FontWeight.Bold,
                                )
                            }
                            SignalPlot(
                                value.waveform,
                                false,
                                Modifier.fillMaxWidth().height(160.dp),
                                strokeColor = tones.primary,
                                glowColor = tones.secondary,
                            )
                        }
                    }
                    Surface(
                        shape = RoundedCornerShape(22.dp),
                        color = MaterialTheme.colorScheme.surfaceContainerLow.copy(alpha = 0.75f),
                        border =
                            BorderStroke(
                                1.dp,
                                MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.35f),
                            ),
                        modifier = Modifier.fillMaxWidth(),
                    ) {
                        Column(Modifier.padding(18.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                            Row(
                                verticalAlignment = Alignment.CenterVertically,
                                horizontalArrangement = Arrangement.spacedBy(8.dp),
                            ) {
                                Icon(
                                    Icons.Default.Speed,
                                    null,
                                    Modifier.size(20.dp),
                                    tint = tones.primary,
                                )
                                Text(
                                    "Spectrum",
                                    style = MaterialTheme.typography.titleLarge,
                                    fontWeight = FontWeight.Bold,
                                )
                            }
                            SignalPlot(
                                value.spectrum.map { ((it + 120f) / 120f).coerceIn(0f, 1f) },
                                false,
                                Modifier.fillMaxWidth().height(160.dp),
                                strokeColor = tones.secondary,
                                glowColor = tones.primary,
                            )
                            Text(
                                "0 – ${((value.spectrum.size - 1) * value.spectrumStepHz).toInt()} Hz",
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                    }
                    Surface(
                        shape = RoundedCornerShape(22.dp),
                        color = MaterialTheme.colorScheme.surfaceContainerLow.copy(alpha = 0.75f),
                        border =
                            BorderStroke(
                                1.dp,
                                MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.35f),
                            ),
                        modifier = Modifier.fillMaxWidth(),
                    ) {
                        Column(Modifier.padding(18.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                            Row(
                                verticalAlignment = Alignment.CenterVertically,
                                horizontalArrangement = Arrangement.spacedBy(8.dp),
                            ) {
                                Icon(
                                    Icons.Default.AutoAwesome,
                                    null,
                                    Modifier.size(20.dp),
                                    tint = tones.primary,
                                )
                                Text(
                                    "Identify this recording",
                                    style = MaterialTheme.typography.titleLarge,
                                    fontWeight = FontWeight.Bold,
                                )
                            }
                            Text(
                                "Recognition sends an audio signature to Shazam. With your AcoustID key it sends the whole-song fingerprint to AcoustID instead.",
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                            OutlinedTextField(
                                key,
                                { key = it },
                                label = { Text("AcoustID key (optional)") },
                                visualTransformation =
                                    androidx.compose.ui.text.input.PasswordVisualTransformation(),
                                modifier = Modifier.fillMaxWidth(),
                                shape = RoundedCornerShape(16.dp),
                                singleLine = true,
                            )
                            TextButton(onClick = { library.features.saveRecognitionKey(key) }) {
                                Text("Save recognition key")
                            }
                            Button(
                                onClick = { track?.let { library.features.recognise(it.id) } },
                                enabled = online && !recognising,
                                shape = RoundedCornerShape(16.dp),
                            ) {
                                Text(if (recognising) "Identifying…" else "Identify by sound")
                            }
                            if (!online)
                                Text(
                                    "Enable online features in Settings to identify recordings.",
                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    style = MaterialTheme.typography.bodySmall,
                                )
                            candidates.forEach { candidate ->
                                Surface(
                                    shape = RoundedCornerShape(16.dp),
                                    color = MaterialTheme.colorScheme.surfaceContainerHighest.copy(alpha = 0.6f),
                                    modifier = Modifier.fillMaxWidth(),
                                ) {
                                    Row(
                                        Modifier.padding(14.dp),
                                        verticalAlignment = Alignment.CenterVertically,
                                    ) {
                                        Column(Modifier.weight(1f)) {
                                            Text(
                                                "${candidate.song.title} · ${candidate.song.artist} · ${candidate.score}%",
                                                style = MaterialTheme.typography.titleMedium,
                                                fontWeight = FontWeight.SemiBold,
                                            )
                                        }
                                        TextButton(
                                            onClick = {
                                                uriHandler.openUri(
                                                    "https://musicbrainz.org/recording/${candidate.song.recording}"
                                                )
                                            }
                                        ) {
                                            Text("View recording")
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
        }
    }
}

@Composable
fun VisualizerScreen(library: LibraryRepository, snapshot: Snapshot?) {
    val samples by library.features.samples.collectAsState()
    val tones by rememberArtworkTones(snapshot?.current, library)
    DisposableEffect(Unit) {
        library.features.visualize(true)
        onDispose { library.features.visualize(false) }
    }
    AmbientBackdrop(tones, Modifier.fillMaxSize(), strength = 0.50f, animated = true) {
        Column(
            Modifier.fillMaxSize().padding(24.dp),
            verticalArrangement = Arrangement.spacedBy(20.dp, Alignment.CenterVertically),
        ) {
            Surface(
                shape = RoundedCornerShape(24.dp),
                color = MaterialTheme.colorScheme.surfaceContainerLow.copy(alpha = 0.75f),
                border = BorderStroke(1.dp, tones.primary.copy(alpha = 0.35f)),
                modifier = Modifier.fillMaxWidth(),
            ) {
                Row(
                    Modifier.padding(16.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(14.dp),
                ) {
                    Artwork(Modifier.size(54.dp), snapshot?.current, library, RoundedCornerShape(14.dp))
                    Column(Modifier.weight(1f)) {
                        Text(
                            snapshot?.current?.title ?: "Play a song to see its sound",
                            style = MaterialTheme.typography.titleLarge,
                            fontWeight = FontWeight.Bold,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                        Row(
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(8.dp),
                        ) {
                            Text(
                                "Live waveform",
                                color = tones.primary,
                                style = MaterialTheme.typography.bodyMedium,
                                fontWeight = FontWeight.Medium,
                            )
                            AudioFormatBadge(snapshot?.current)
                        }
                    }
                }
            }
            Surface(
                shape = RoundedCornerShape(26.dp),
                color = MaterialTheme.colorScheme.surfaceContainerLow.copy(alpha = 0.75f),
                border = BorderStroke(1.2.dp, tones.primary.copy(alpha = 0.40f)),
                modifier = Modifier.fillMaxWidth(),
            ) {
                SignalPlot(
                    samples,
                    true,
                    Modifier.fillMaxWidth().height(320.dp),
                    strokeColor = tones.primary,
                    glowColor = tones.secondary,
                )
            }
        }
    }
}

@Composable
private fun SignalPlot(
    values: List<Float>,
    centered: Boolean,
    modifier: Modifier,
    strokeColor: Color = MaterialTheme.colorScheme.primary,
    glowColor: Color = MaterialTheme.colorScheme.secondary,
) {
    Canvas(modifier) {
        val width = size.width
        val height = size.height
        val gridColor = strokeColor.copy(alpha = 0.12f)
        val centerLineColor = strokeColor.copy(alpha = 0.25f)
        drawLine(
            gridColor,
            Offset(0f, height * 0.25f),
            Offset(width, height * 0.25f),
            strokeWidth = 1.dp.toPx(),
        )
        drawLine(
            if (centered) centerLineColor else gridColor,
            Offset(0f, height * 0.5f),
            Offset(width, height * 0.5f),
            strokeWidth = if (centered) 1.5.dp.toPx() else 1.dp.toPx(),
        )
        drawLine(
            gridColor,
            Offset(0f, height * 0.75f),
            Offset(width, height * 0.75f),
            strokeWidth = 1.dp.toPx(),
        )
        if (values.size < 2) return@Canvas
        val path = Path()
        values.forEachIndexed { index, raw ->
            val value = raw.takeIf { it.isFinite() } ?: 0f
            val x = width * index / (values.size - 1)
            val y =
                if (centered) height * (0.5f - value.coerceIn(-1f, 1f) * 0.45f)
                else height * (1f - value.coerceIn(0f, 1f))
            if (index == 0) path.moveTo(x, y) else path.lineTo(x, y)
        }
        val gradientBrush =
            Brush.horizontalGradient(
                listOf(strokeColor, glowColor, strokeColor)
            )
        drawPath(path, strokeColor.copy(alpha = 0.25f), style = Stroke(6.dp.toPx()))
        drawPath(path, gradientBrush, style = Stroke(2.2.dp.toPx()))
    }
}

@Composable
fun LyricsScreen(
    library: LibraryRepository,
    track: Track?,
    snapshot: Snapshot?,
    controller: MediaController?,
) {
    val sheet by library.features.lyrics.collectAsState()
    val frame by library.features.lyricFrame.collectAsState()
    val loading by library.features.lyricsLoading.collectAsState()
    val online by library.features.online.collectAsState()
    val list = rememberLazyListState()
    val tones by rememberArtworkTones(track, library)
    LaunchedEffect(track?.id) { track?.let { library.features.loadLyrics(it.id) } }
    val active = frame?.line?.toInt()
    LaunchedEffect(active) {
        active?.let {
            if (!list.isScrollInProgress) list.animateScrollToItem((it - 2).coerceAtLeast(0))
        }
    }
    AmbientBackdrop(tones, Modifier.fillMaxSize(), strength = 0.55f, animated = true) {
        Column(Modifier.fillMaxSize()) {
            Surface(
                shape = RoundedCornerShape(22.dp),
                color = MaterialTheme.colorScheme.surfaceContainerHighest.copy(alpha = 0.75f),
                border = BorderStroke(1.dp, tones.primary.copy(alpha = 0.35f)),
                modifier =
                    Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp),
            ) {
                Row(
                    Modifier.fillMaxWidth().padding(horizontal = 14.dp, vertical = 8.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Artwork(Modifier.size(42.dp), track, library, RoundedCornerShape(10.dp))
                    Column(Modifier.weight(1f).padding(horizontal = 12.dp)) {
                        Text(
                            track?.title ?: "Lyrics",
                            style = MaterialTheme.typography.titleMedium,
                            fontWeight = FontWeight.Bold,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                        Text(
                            track?.artist.orEmpty().ifBlank { "Synchronized lyrics" },
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                    }
                    if (online && track != null)
                        FilledTonalButton(
                            onClick = { library.features.loadLyrics(track.id, true) },
                            shape = RoundedCornerShape(14.dp),
                            contentPadding = PaddingValues(horizontal = 12.dp, vertical = 4.dp),
                        ) {
                            Icon(Icons.Default.Refresh, null, Modifier.size(16.dp))
                            Spacer(Modifier.size(4.dp))
                            Text("Refresh")
                        }
                }
            }
            if (loading)
                LinearProgressIndicator(
                    Modifier.fillMaxWidth(),
                    strokeCap = StrokeCap.Round,
                    color = tones.primary,
                )
            val lines = sheet?.takeIf { it.trackId == track?.id }?.lines.orEmpty()
            if (lines.isEmpty() && !loading)
                Box(Modifier.fillMaxSize().padding(32.dp), contentAlignment = Alignment.Center) {
                    Surface(
                        shape = RoundedCornerShape(24.dp),
                        color = MaterialTheme.colorScheme.surfaceContainerLow.copy(alpha = 0.8f),
                        border =
                            BorderStroke(
                                1.dp,
                                MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.35f),
                            ),
                        modifier = Modifier.padding(16.dp),
                    ) {
                        Column(
                            Modifier.padding(24.dp),
                            horizontalAlignment = Alignment.CenterHorizontally,
                            verticalArrangement = Arrangement.spacedBy(12.dp),
                        ) {
                            Icon(
                                Icons.Default.Lyrics,
                                null,
                                Modifier.size(48.dp),
                                tint = tones.primary,
                            )
                            Text(
                                if (online) "No lyrics found for this song"
                                else
                                    "No saved lyrics. Enable online features to look them up, or add an LRC beside the song.",
                                style = MaterialTheme.typography.bodyMedium,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                    }
                }
            LazyColumn(
                state = list,
                modifier = Modifier.weight(1f),
                verticalArrangement = Arrangement.spacedBy(28.dp),
                contentPadding = PaddingValues(top = 40.dp, bottom = 180.dp),
            ) {
                itemsIndexed(lines) { index, line ->
                    val timed = line.atMs != null
                    val lit = !timed || frame?.active?.contains(index.toUInt()) == true
                    val scale by
                        animateFloatAsState(
                            if (lit) 1.06f else 0.94f,
                            spring(
                                dampingRatio = Spring.DampingRatioLowBouncy,
                                stiffness = Spring.StiffnessLow,
                            ),
                            label = "Lyric emphasis",
                        )
                    val alpha by
                        animateFloatAsState(
                            if (lit) 1f
                            else if (active == null || abs(index - active) < 3) 0.52f else 0.22f,
                            label = "Lyric focus",
                        )
                    val color by
                        animateColorAsState(
                            if (lit && timed) tones.primary
                            else MaterialTheme.colorScheme.onSurface,
                            label = "Sung line",
                        )
                    val position = snapshot?.positionMs ?: 0uL
                    val text = buildAnnotatedString {
                        if (line.words.isEmpty() || !lit) append(line.text)
                        else
                            line.words.forEach { word ->
                                withStyle(
                                    SpanStyle(
                                        color =
                                            if (position >= word.atMs) color
                                            else color.copy(alpha = 0.4f)
                                    )
                                ) {
                                    append(word.text)
                                }
                            }
                    }
                    Text(
                        text,
                        Modifier.fillMaxWidth()
                            .padding(horizontal = 24.dp)
                            .graphicsLayer {
                                scaleX = scale
                                scaleY = scale
                                this.alpha = alpha
                            }
                            .blur(
                                if (timed && active != null && abs(index - active) > 3) 0.8.dp
                                else 0.dp
                            )
                            .clickable(enabled = timed) {
                                line.atMs?.let { controller?.seekTo(it.toLong()) }
                            },
                        color = color,
                        style = MaterialTheme.typography.headlineMedium,
                        fontWeight = if (lit) FontWeight.Bold else FontWeight.Medium,
                    )
                }
            }
            sheet
                ?.origin
                ?.takeIf { it.isNotBlank() }
                ?.let {
                    Text(
                        "Lyrics: $it",
                        Modifier.padding(horizontal = 20.dp, vertical = 8.dp),
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
        }
    }
}

@Composable
fun OnlineScreen(library: LibraryRepository, openSettings: () -> Unit) {
    var download by remember { mutableStateOf<org.resonate.android.nativecore.SearchSong?>(null) }
    var mode by remember { mutableStateOf("Songs") }
    var heading by remember { mutableStateOf("") }
    var importName by remember { mutableStateOf<String?>(null) }
    val collection by library.features.onlineCollection.collectAsState()
    LaunchedEffect(collection) {
        collection?.let { opened ->
            heading = opened.name.ifBlank { "Shared music" }
            mode =
                when {
                    opened.releases.isNotEmpty() -> "Releases"
                    opened.artists.isNotEmpty() -> "Artists"
                    else -> "Album songs"
                }
        }
    }
    var profile by remember { mutableStateOf(false) }
    val downloading by library.features.downloading.collectAsState()
    val downloadedBytes by library.features.downloadedBytes.collectAsState()
    val downloadStatus by library.features.downloadStatus.collectAsState()
    LaunchedEffect(Unit) { library.features.refreshFolders() }
    val enabled by library.features.online.collectAsState()
    val songs by library.features.onlineSongs.collectAsState()
    val albums by library.features.onlineAlbums.collectAsState()
    val artists by library.features.onlineArtists.collectAsState()
    val releases by library.features.onlineReleases.collectAsState()
    val nextAlbums by library.features.artistAlbumsNext.collectAsState()
    var discographyArtist by remember { mutableStateOf<String?>(null) }
    val artist by library.features.artist.collectAsState()
    val searching by library.features.searching.collectAsState()
    var query by remember { mutableStateOf("") }
    Column(
        Modifier.fillMaxSize().padding(horizontal = 20.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Text("Online music search", style = MaterialTheme.typography.headlineSmall)
        if (!enabled) {
            Text(
                "Online features are disabled. Turn them on in Settings to search and download music."
            )
            TextButton(onClick = openSettings) { Text("Open Settings") }
        }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            listOf("Songs", "Albums", "Artists", "Link").forEach { label ->
                FilterChip(
                    mode == label,
                    {
                        mode = label
                        profile = false
                        library.features.clearCollection()
                    },
                    label = { Text(label) },
                )
            }
        }
        OutlinedTextField(
            query,
            { query = it },
            label = {
                Text(
                    if (mode == "Link") "Album, artist, song or playlist link"
                    else "Song, album or artist"
                )
            },
            modifier = Modifier.fillMaxWidth(),
            singleLine = true,
        )
        Button(
            onClick = {
                profile = false
                discographyArtist = null
                val address = query.trim()
                if (
                    mode == "Link" ||
                        listOf("https://", "http://", "spotify:").any {
                            address.startsWith(it, ignoreCase = true)
                        }
                )
                    library.features.openLink(address)
                else
                    when (mode) {
                        "Albums" -> library.features.searchAlbums(query)
                        "Artists" -> library.features.searchArtists(query)
                        else -> {
                            mode = "Songs"
                            library.features.searchOnline(query)
                        }
                    }
            },
            enabled = enabled && query.length >= 3 && !searching,
        ) {
            Text("Search online")
        }
        if (searching) LinearProgressIndicator(Modifier.fillMaxWidth())
        downloadStatus?.let { Text(it, style = MaterialTheme.typography.bodySmall) }
        if (downloading) {
            Text("${downloadedBytes / 1024uL} KiB received")
            TextButton(onClick = library.features::cancelDownload) { Text("Cancel download") }
        }
        if (!searching)
            LazyColumn {
                if (mode == "Releases" || mode == "Album songs")
                    item {
                        Text(heading, style = MaterialTheme.typography.titleLarge)
                        if (mode == "Album songs" && songs.isNotEmpty()) {
                            TextButton(onClick = { library.features.saveWanted(songs) }) {
                                Text("Save all for later")
                            }
                            if (collection?.name?.isNotBlank() == true)
                                TextButton(onClick = { importName = collection?.name }) {
                                    Text("Import local matches as playlist")
                                }
                        }
                    }
                when (mode) {
                    "Albums" -> {
                        items(albums, key = { it.id }) { album ->
                            Column(
                                Modifier.fillMaxWidth()
                                    .clickable {
                                        heading = album.title
                                        mode = "Releases"
                                        library.features.onlineReleases(album.id)
                                    }
                                    .padding(vertical = 16.dp)
                            ) {
                                Text(album.title, style = MaterialTheme.typography.titleMedium)
                                if (album.artist.isNotBlank()) Text(album.artist)
                                Text(album.date, color = MaterialTheme.colorScheme.onSurfaceVariant)
                            }
                        }
                        nextAlbums?.let { from ->
                            discographyArtist?.let { id ->
                                item {
                                    TextButton(
                                        onClick = { library.features.artistAlbums(id, from) }
                                    ) {
                                        Text("More releases")
                                    }
                                }
                            }
                        }
                    }
                    "Artists" -> {
                        if (profile) artist?.let { info -> item { ArtistInformationCard(info) } }
                        items(artists, key = { it.id }) { candidate ->
                            Column(Modifier.fillMaxWidth().padding(vertical = 16.dp)) {
                                Text(candidate.name, style = MaterialTheme.typography.titleMedium)
                                Text(candidate.kind)
                                TextButton(
                                    onClick = {
                                        profile = true
                                        library.features.onlineArtist(candidate.id)
                                    }
                                ) {
                                    Text("Artist details")
                                }
                                TextButton(
                                    onClick = {
                                        query = candidate.name
                                        mode = "Albums"
                                        discographyArtist = candidate.id
                                        library.features.artistAlbums(candidate.id)
                                    }
                                ) {
                                    Text("Find albums")
                                }
                            }
                        }
                    }
                    "Releases" ->
                        items(releases, key = { it.id }) { release ->
                            Column(
                                Modifier.fillMaxWidth()
                                    .clickable {
                                        heading = release.title
                                        mode = "Album songs"
                                        library.features.releaseSongs(release.id)
                                    }
                                    .padding(vertical = 16.dp)
                            ) {
                                Text(release.title, style = MaterialTheme.typography.titleMedium)
                                Text(
                                    listOf(
                                            release.date,
                                            release.country,
                                            if (release.songs > 0u) "${release.songs} songs"
                                            else "View track list",
                                        )
                                        .filter { it.isNotBlank() }
                                        .joinToString(" · ")
                                )
                            }
                        }
                    else ->
                        itemsIndexed(songs, key = { index, song -> "$index:${song.recording}" }) {
                            _,
                            song ->
                            Column(Modifier.fillMaxWidth().padding(vertical = 16.dp)) {
                                Text(song.title, style = MaterialTheme.typography.titleMedium)
                                Text(song.artist)
                                Text(song.album, color = MaterialTheme.colorScheme.onSurfaceVariant)
                                song.durationMs?.let {
                                    Text(timeLabel(it), style = MaterialTheme.typography.bodySmall)
                                }
                                Row {
                                    TextButton(
                                        onClick = { download = song },
                                        enabled = !downloading && enabled,
                                    ) {
                                        Text("Download")
                                    }
                                    TextButton(
                                        onClick = { library.features.saveWanted(listOf(song)) }
                                    ) {
                                        Text("Save for later")
                                    }
                                }
                            }
                        }
                }
            }
    }
    download?.let { song -> DownloadFolderDialog(library, song) { download = null } }
    importName?.let { name ->
        AlertDialog(
            onDismissRequest = { importName = null },
            title = { Text("Import matching local songs") },
            text = {
                Column {
                    Text(
                        "Songs already in your library are added in the shared order. Save the remaining recordings for later to download them."
                    )
                    OutlinedTextField(
                        name,
                        { importName = it },
                        label = { Text("Playlist name") },
                        singleLine = true,
                    )
                }
            },
            confirmButton = {
                TextButton(
                    onClick = {
                        library.features.importPlaylist(name, songs)
                        importName = null
                    },
                    enabled = name.isNotBlank(),
                ) {
                    Text("Import")
                }
            },
            dismissButton = { TextButton(onClick = { importName = null }) { Text("Cancel") } },
        )
    }
}

@Composable
private fun ArtistInformationCard(info: org.resonate.android.nativecore.ArtistInformation) {
    val uriHandler = androidx.compose.ui.platform.LocalUriHandler.current
    val bitmap by
        androidx.compose.runtime.produceState<android.graphics.Bitmap?>(null, info) {
            value =
                kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.IO) {
                    info.picture?.let { bytes ->
                        val options =
                            android.graphics.BitmapFactory.Options().apply {
                                inJustDecodeBounds = true
                            }
                        android.graphics.BitmapFactory.decodeByteArray(
                            bytes,
                            0,
                            bytes.size,
                            options,
                        )
                        if (options.outWidth !in 1..32768 || options.outHeight !in 1..32768)
                            return@let null
                        options.inJustDecodeBounds = false
                        options.inSampleSize = 1
                        while (
                            options.outWidth / options.inSampleSize > 1024 ||
                                options.outHeight / options.inSampleSize > 1024
                        ) options.inSampleSize *= 2
                        android.graphics.BitmapFactory.decodeByteArray(
                            bytes,
                            0,
                            bytes.size,
                            options,
                        )
                    }
                }
        }
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        bitmap?.let {
            androidx.compose.foundation.Image(
                it.asImageBitmap(),
                null,
                Modifier.fillMaxWidth().height(200.dp),
                contentScale = androidx.compose.ui.layout.ContentScale.Crop,
            )
        }
        Text(info.name, style = MaterialTheme.typography.headlineSmall)
        Text(
            listOf(info.kind, info.country, info.area)
                .filter { it.isNotBlank() }
                .joinToString(" · ")
        )
        Text(info.genres.joinToString(" · "))
        info.links.forEach { link ->
            TextButton(onClick = { uriHandler.openUri(link) }) { Text(link) }
        }
    }
}

private fun timeLabel(milliseconds: ULong): String {
    val seconds = milliseconds / 1000uL
    return "${seconds / 60uL}:${(seconds % 60uL).toString().padStart(2, '0')}"
}
