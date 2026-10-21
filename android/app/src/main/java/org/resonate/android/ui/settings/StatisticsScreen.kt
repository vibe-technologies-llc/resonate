package org.resonate.android.ui.settings

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Album
import androidx.compose.material.icons.rounded.BarChart
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.FilterChip
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.getValue
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewModelScope
import java.util.TimeZone
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.resonate.android.nativecore.LibraryBreakdown
import org.resonate.android.nativecore.LibraryShare
import org.resonate.android.nativecore.LibraryStatistics
import org.resonate.android.nativecore.ListenedOnDay
import org.resonate.android.nativecore.ListenedTo
import org.resonate.android.nativecore.ListeningDetail
import org.resonate.android.nativecore.Track
import org.resonate.android.ui.LocalResonate
import org.resonate.android.ui.Resonate
import org.resonate.android.ui.albumOwner
import org.resonate.android.ui.components.ArtistPortrait
import org.resonate.android.ui.components.EmptyState
import org.resonate.android.ui.components.LocalSheets
import org.resonate.android.ui.components.Page
import org.resonate.android.ui.components.SectionHeader
import org.resonate.android.ui.components.Sizes
import org.resonate.android.ui.components.SongArtwork
import org.resonate.android.ui.components.TrackRow
import org.resonate.android.ui.navigation.LocalNavigator
import org.resonate.android.ui.navigation.Screen
import org.resonate.android.ui.resonateViewModel

data class Ranked(val entry: ListenedTo, val artworkSong: ULong?)

data class Listening(
    val period: Period,
    val totals: LibraryStatistics,
    val detail: ListeningDetail?,
    val albums: List<Ranked>,
    val artists: List<Ranked>,
)

sealed interface StatisticsState {
    data object Loading : StatisticsState

    data class Failed(val message: String) : StatisticsState

    data class Ready(val listening: Listening) : StatisticsState
}

private const val MOST_LISTENED_SHOWN = 5u
private const val TOP_SONGS_FIRST_SHOWN = 10
private const val DAYS_DRAWN = 60

private fun albumSong(entry: ListenedTo, tracks: List<Track>): ULong? =
    (tracks.firstOrNull {
            it.album.equals(entry.name, ignoreCase = true) &&
                it.albumOwner.equals(entry.by, ignoreCase = true)
        } ?: tracks.firstOrNull { it.album.equals(entry.name, ignoreCase = true) })
        ?.id

private fun artistSong(entry: ListenedTo, tracks: List<Track>): ULong? =
    (tracks.firstOrNull { it.artist.equals(entry.name, ignoreCase = true) }
            ?: tracks.firstOrNull { it.albumArtist.equals(entry.name, ignoreCase = true) })
        ?.id

class StatisticsViewModel(private val resonate: Resonate) : ViewModel() {
    private val mutableState = MutableStateFlow<StatisticsState>(StatisticsState.Loading)
    val state = mutableState.asStateFlow()
    private val mutableBreakdown = MutableStateFlow<LibraryBreakdown?>(null)
    val breakdown = mutableBreakdown.asStateFlow()
    private var loading: Job? = null

    fun load(period: Period) {
        val shown = (mutableState.value as? StatisticsState.Ready)?.listening?.period
        if (shown == period) return
        loading?.cancel()
        if (mutableState.value !is StatisticsState.Ready) mutableState.value = StatisticsState.Loading
        loading = viewModelScope.launch {
            val zone = TimeZone.getDefault()
            val offset = zone.getOffset(System.currentTimeMillis()) / 1000
            val read =
                resonate.read { native ->
                    withContext(Dispatchers.IO) {
                        val totals = native.statistics(period.days)
                        val detail =
                            try {
                                native.listeningDetail(period.days, MOST_LISTENED_SHOWN, zone.id, offset)
                            } catch (failure: Exception) {
                                android.util.Log.w("Resonate", "Listening detail unavailable", failure)
                                null
                            }
                        totals to detail
                    }
                }
            mutableState.value =
                read.fold(
                    onSuccess = { (totals, detail) ->
                        val tracks = resonate.library.allTracks.value
                        val ranked =
                            withContext(Dispatchers.Default) {
                                (detail?.albums.orEmpty().map { Ranked(it, albumSong(it, tracks)) }) to
                                    detail?.artists.orEmpty().map { Ranked(it, artistSong(it, tracks)) }
                            }
                        StatisticsState.Ready(
                            Listening(period, totals, detail, ranked.first, ranked.second)
                        )
                    },
                    onFailure = {
                        StatisticsState.Failed(it.message ?: "Statistics could not be read")
                    },
                )
        }
        if (mutableBreakdown.value == null)
            viewModelScope.launch {
                resonate
                    .read { native -> withContext(Dispatchers.IO) { native.libraryBreakdown() } }
                    .onSuccess { mutableBreakdown.value = it }
            }
    }

    fun retry(period: Period) {
        mutableState.value = StatisticsState.Loading
        load(period)
    }
}

enum class BreakdownKind(val label: String) {
    FORMATS("Formats"),
    RATES("Sample rates"),
    GENRES("Genres"),
    DECADES("Decades"),
}

private fun LibraryBreakdown.of(kind: BreakdownKind): List<LibraryShare> =
    when (kind) {
        BreakdownKind.FORMATS -> formats
        BreakdownKind.RATES -> rates
        BreakdownKind.GENRES -> genres
        BreakdownKind.DECADES -> decades
    }

@Composable
fun StatisticsScreen() {
    val model = resonateViewModel { StatisticsViewModel(it) }
    var period by rememberSaveable { mutableStateOf(Period.ALL_TIME) }
    var allSongs by rememberSaveable { mutableStateOf(false) }
    var kind by rememberSaveable { mutableStateOf(BreakdownKind.FORMATS) }
    val state by model.state.collectAsStateWithLifecycle()
    val breakdown by model.breakdown.collectAsStateWithLifecycle()
    val resonate = LocalResonate.current
    val sheets = LocalSheets.current
    val navigator = LocalNavigator.current
    LaunchedEffect(period) { model.load(period) }
    Page("Listening statistics") { padding ->
        SettingsList(padding, Modifier.testTag("statistics-list")) {
            item(key = "periods") { Periods(period) { period = it } }
            when (val current = state) {
                StatisticsState.Loading -> item(key = "loading") { ListLoading() }
                is StatisticsState.Failed ->
                    item(key = "failed") {
                        EmptyState(
                            Icons.Rounded.BarChart,
                            "Statistics could not be read",
                            detail = current.message,
                            action = "Try again",
                            onAction = { model.retry(period) },
                        )
                    }
                is StatisticsState.Ready -> {
                    val listening = current.listening
                    val totals = listening.totals
                    item(key = "listening-header") { SectionHeader("Your listening") }
                    item(key = "listening") { ListeningFigures(listening) }
                    val days = listening.detail?.days.orEmpty()
                    if (days.count { it.listenedMs > 0uL } > 1) {
                        item(key = "days") { DaysChart(daysShown(days, DAYS_DRAWN)) }
                    }
                    if (totals.topTracks.isNotEmpty()) {
                        val shown =
                            if (allSongs) totals.topTracks
                            else totals.topTracks.take(TOP_SONGS_FIRST_SHOWN)
                        item(key = "top-header") {
                            SectionHeader(
                                "Top songs",
                                action =
                                    if (totals.topTracks.size > TOP_SONGS_FIRST_SHOWN)
                                        if (allSongs) "Show fewer" else "Show all"
                                    else null,
                                onAction = { allSongs = !allSongs },
                            )
                        }
                        itemsIndexed(shown, key = { _, track -> "top-${track.id}" }) { index, track ->
                            TrackRow(
                                track = track,
                                onClick = { resonate.playback.play(totals.topTracks, index) },
                                onMore = { sheets.song(track) },
                                modifier = Modifier.testTag("top-song"),
                            )
                        }
                    }
                    if (listening.albums.isNotEmpty()) {
                        item(key = "albums-header") { SectionHeader("Albums you played most") }
                        itemsIndexed(listening.albums, key = { _, it -> "album-${it.entry.key}" }) { _, ranked ->
                            RankedRow(ranked, circle = false) {
                                navigator.open(
                                    Screen.Album(
                                        ranked.entry.key,
                                        ranked.entry.name,
                                        ranked.entry.by,
                                        ranked.artworkSong,
                                    )
                                )
                            }
                        }
                    }
                    if (listening.artists.isNotEmpty()) {
                        item(key = "artists-header") { SectionHeader("Artists you played most") }
                        itemsIndexed(listening.artists, key = { _, it -> "artist-${it.entry.key}" }) { _, ranked ->
                            RankedRow(ranked, circle = true) {
                                navigator.open(Screen.Artist(ranked.entry.name, ranked.artworkSong))
                            }
                        }
                    }
                    if (totals.plays == 0uL) {
                        item(key = "nothing") {
                            NoticeCard(
                                Icons.Rounded.BarChart,
                                "Nothing played ${listening.period.within} yet. Songs count once you have heard half of them, or four minutes.",
                            )
                        }
                    }
                    item(key = "library-header") { SectionHeader("Your library") }
                    item(key = "library") { LibraryFigures(totals) }
                    breakdown?.let { found ->
                        item(key = "kinds") { Kinds(kind) { kind = it } }
                        val shares = found.of(kind)
                        val whole = shareWhole(shares, totals.songs)
                        if (shares.isEmpty()) {
                            item(key = "no-shares") { Note("Nothing to show yet.") }
                        }
                        itemsIndexed(shares, key = { _, it -> "${kind.name}-${it.name}" }) { _, share ->
                            ShareRow(share, whole)
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun Periods(selected: Period, onSelect: (Period) -> Unit) {
    SingleChoiceSegmentedButtonRow(
        Modifier.fillMaxWidth()
            .padding(horizontal = Sizes.gutter, vertical = 8.dp)
            .testTag("periods")
    ) {
        Period.entries.forEachIndexed { index, period ->
            SegmentedButton(
                selected = period == selected,
                onClick = { onSelect(period) },
                shape = SegmentedButtonDefaults.itemShape(index, Period.entries.size),
                contentPadding = PaddingValues(horizontal = 4.dp, vertical = 10.dp),
                icon = {},
            ) {
                Text(period.label, maxLines = 1, textAlign = TextAlign.Center)
            }
        }
    }
}

@Composable
private fun FigureTile(label: String, figure: Figure, modifier: Modifier = Modifier) {
    Card(
        modifier = modifier,
        shape = MaterialTheme.shapes.large,
        colors =
            CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerHigh),
    ) {
        Column(
            Modifier.padding(16.dp).semantics(mergeDescendants = true) {
                contentDescription = "$label: ${figure.value} ${figure.unit}"
            }
        ) {
            Row(verticalAlignment = Alignment.Bottom) {
                Text(
                    figure.value,
                    style = MaterialTheme.typography.headlineMedium,
                    color = MaterialTheme.colorScheme.primary,
                    maxLines = 1,
                )
                if (figure.unit.isNotEmpty()) {
                    Spacer(Modifier.padding(start = 4.dp))
                    Text(
                        figure.unit,
                        style = MaterialTheme.typography.titleSmall,
                        color = MaterialTheme.colorScheme.primary,
                        modifier = Modifier.padding(bottom = 4.dp),
                    )
                }
            }
            Text(
                label,
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

@Composable
private fun Figures(tiles: List<Pair<String, Figure>>) {
    Column(
        Modifier.fillMaxWidth().padding(horizontal = Sizes.gutter, vertical = 4.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        tiles.chunked(2).forEach { pair ->
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                pair.forEach { (label, figure) ->
                    FigureTile(label, figure, Modifier.weight(1f))
                }
            }
        }
    }
}

private fun count(value: ULong, unit: String = "") = Figure(grouped(value), unit)

@Composable
private fun ListeningFigures(listening: Listening) {
    val totals = listening.totals
    val tiles =
        listOfNotNull(
            "Listens" to count(totals.plays),
            "Time listening" to durationFigure(totals.listenedMs),
            listening.detail?.let { "Different songs" to count(it.songsHeard) },
        )
    Figures(tiles)
    listening.detail?.firstHeard?.let {
        Footnote("Counting since ${dayLabel(it)}")
    }
}

@Composable
private fun LibraryFigures(totals: LibraryStatistics) {
    Figures(
        listOf(
            "Songs" to count(totals.songs),
            "Albums" to count(totals.albums),
            "Artists" to count(totals.artists),
            "To listen through" to durationFigure(totals.durationMs),
        )
    )
}

private val MAX_DAY_BAR_WIDTH = 24.dp
private val MAX_DAY_BAR_CORNER = 6.dp

@Composable
private fun DaysChart(days: List<ListenedOnDay>) {
    val scheme = MaterialTheme.colorScheme
    val most = days.maxOf { it.listenedMs }.coerceAtLeast(1uL)
    val busiest = busiestDay(days)
    val bar = scheme.primary
    val track = scheme.surfaceContainerHighest
    Card(
        modifier = Modifier.fillMaxWidth().padding(horizontal = Sizes.gutter, vertical = 8.dp),
        shape = MaterialTheme.shapes.large,
        colors = CardDefaults.cardColors(containerColor = scheme.surfaceContainerLow),
    ) {
        Column(Modifier.padding(16.dp)) {
            Text("Listening by day", style = MaterialTheme.typography.titleSmall)
            busiest?.let {
                Text(
                    "Most on ${dayLabel(it.starts)} · ${listenedLine(it.plays, it.listenedMs)}",
                    style = MaterialTheme.typography.bodySmall,
                    color = scheme.onSurfaceVariant,
                )
            }
            Spacer(Modifier.height(12.dp))
            Canvas(
                Modifier.fillMaxWidth().height(96.dp).semantics {
                    contentDescription =
                        "Listening on ${days.size} days" +
                            (busiest?.let { ", most on ${dayLabel(it.starts)}" } ?: "")
                }
            ) {
                val slot = size.width / days.size
                val gap = (slot * 0.25f).coerceAtMost(4.dp.toPx())
                val width = (slot - gap).coerceIn(1f, MAX_DAY_BAR_WIDTH.toPx())
                val corner = (width / 2f).coerceAtMost(MAX_DAY_BAR_CORNER.toPx())
                val radius = CornerRadius(corner, corner)
                days.forEachIndexed { index, day ->
                    val x = index * slot + (slot - width) / 2f
                    drawRoundRect(track, Offset(x, 0f), Size(width, size.height), radius)
                    val share = day.listenedMs.toFloat() / most.toFloat()
                    val height =
                        (size.height * share).coerceAtLeast(if (day.listenedMs > 0uL) corner * 2f else 0f)
                    if (height > 0f)
                        drawRoundRect(bar, Offset(x, size.height - height), Size(width, height), radius)
                }
            }
            Spacer(Modifier.height(8.dp))
            Row {
                Text(
                    dayLabel(days.first().starts),
                    style = MaterialTheme.typography.labelSmall,
                    color = scheme.onSurfaceVariant,
                    modifier = Modifier.weight(1f),
                )
                Text(
                    dayLabel(days.last().starts),
                    style = MaterialTheme.typography.labelSmall,
                    color = scheme.onSurfaceVariant,
                )
            }
        }
    }
}

@Composable
private fun RankedRow(ranked: Ranked, circle: Boolean, onClick: () -> Unit) {
    val entry = ranked.entry
    ListItem(
        onClick = onClick,
        leadingContent = {
            if (circle) ArtistPortrait(ranked.artworkSong, Sizes.rowArt)
            else
                SongArtwork(
                    ranked.artworkSong,
                    Sizes.rowArt,
                    shape = MaterialTheme.shapes.medium,
                    placeholder = Icons.Rounded.Album,
                )
        },
        supportingContent = {
            Text(
                listOfNotNull(
                        entry.by.takeIf { !circle && it.isNotBlank() },
                        listenedLine(entry.plays, entry.listenedMs),
                    )
                    .joinToString(" · "),
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
        },
        colors = ListItemDefaults.colors(containerColor = Color.Transparent),
    ) {
        Text(entry.name.ifBlank { if (circle) "Unknown artist" else "Unknown album" }, maxLines = 1, overflow = TextOverflow.Ellipsis)
    }
}

@Composable
private fun Kinds(selected: BreakdownKind, onSelect: (BreakdownKind) -> Unit) {
    Row(
        Modifier.fillMaxWidth()
            .horizontalScroll(rememberScrollState())
            .padding(horizontal = Sizes.gutter, vertical = 8.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        BreakdownKind.entries.forEach { kind ->
            FilterChip(
                selected = kind == selected,
                onClick = { onSelect(kind) },
                label = { Text(kind.label) },
            )
        }
    }
}

@Composable
private fun ShareRow(share: LibraryShare, whole: ULong) {
    val figure = durationFigure(share.durationMs)
    Column(
        Modifier.fillMaxWidth()
            .padding(horizontal = Sizes.gutter, vertical = 8.dp)
            .semantics(mergeDescendants = true) {}
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(
                share.name.ifBlank { "Unknown" },
                style = MaterialTheme.typography.bodyLarge,
                modifier = Modifier.weight(1f),
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            Text(
                percent(share.songs, whole),
                style = MaterialTheme.typography.labelLarge,
                color = MaterialTheme.colorScheme.primary,
            )
        }
        Spacer(Modifier.height(6.dp))
        LinearProgressIndicator(
            progress = { shareOf(share.songs, whole) },
            modifier = Modifier.fillMaxWidth(),
            drawStopIndicator = {},
        )
        Spacer(Modifier.height(4.dp))
        Text(
            "${grouped(share.songs)} ${if (share.songs == 1uL) "song" else "songs"} · ${figure.value} ${figure.unit}",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}
