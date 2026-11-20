package org.resonate.android.ui.library

import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.PlaylistAdd
import androidx.compose.material.icons.automirrored.rounded.PlaylistPlay
import androidx.compose.material.icons.automirrored.rounded.QueueMusic
import androidx.compose.material3.MaterialShapes
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.toShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.resonate.android.DownloadPlan
import org.resonate.android.R
import org.resonate.android.heldKey
import org.resonate.android.nativecore.ExploreScope
import org.resonate.android.nativecore.SearchSong
import org.resonate.android.nativecore.Track
import org.resonate.android.ui.LocalResonate
import org.resonate.android.ui.Mix
import org.resonate.android.ui.Resonate
import org.resonate.android.ui.SuggestedSearch
import org.resonate.android.ui.components.LocalSheets
import org.resonate.android.ui.components.PlaylistCover
import org.resonate.android.ui.components.SongSelectionBar
import org.resonate.android.ui.components.TrackRow
import org.resonate.android.ui.components.rememberSelection
import org.resonate.android.ui.navigation.Screen
import org.resonate.android.ui.resonateViewModel
import org.resonate.android.ui.search.DownloadCard
import org.resonate.android.ui.search.DownloadChooser
import org.resonate.android.ui.search.Downloads
import org.resonate.android.words

data class MixPage(val tracks: List<Track>, val covers: List<ULong>)

class FavouritesViewModel(resonate: Resonate) : ViewModel() {
    private val shared = SharingStarted.WhileSubscribed(5_000)

    val tracks: StateFlow<Loaded<List<Track>>> =
        resonate.library.allTracks
            .map { songs -> Loaded.Ready(songs.filter { it.favourite }) }
            .stateIn(viewModelScope, shared, Loaded.Ready(resonate.library.allTracks.value.filter { it.favourite }))

    val playing: StateFlow<ULong?> = resonate.playingId().stateIn(viewModelScope, shared, null)
}

class MixViewModel(private val resonate: Resonate, mix: Mix) : ViewModel() {
    private val shared = SharingStarted.WhileSubscribed(5_000)

    val retry = Retry()

    val page: StateFlow<Loaded<MixPage>> =
        combine(
                resonate.catalogRead(retry, follow = false) { native ->
                    val filter = mix.filter()
                    MixPage(filteredFully(native, filter), native.picturedBy(filter))
                },
                resonate.library.allTracks,
            ) { loaded, _ ->
                when (loaded) {
                    is Loaded.Ready -> Loaded.Ready(loaded.value.copy(tracks = resonate.current(loaded.value.tracks)))
                    else -> loaded
                }
            }
            .stateIn(viewModelScope, shared, Loaded.Loading)

    val playing: StateFlow<ULong?> = resonate.playingId().stateIn(viewModelScope, shared, null)
}

class SuggestedViewModel(private val resonate: Resonate, search: SuggestedSearch) : ViewModel() {
    private val shared = SharingStarted.WhileSubscribed(5_000)

    val retry = Retry()

    val page: StateFlow<Loaded<MixPage>> =
        combine(
                resonate.catalogRead(retry, follow = false) { native ->
                    val asked = search.native()
                    MixPage(
                        savedSearchFully(native, asked),
                        native.measureSavedSearch(asked).covers,
                    )
                },
                resonate.library.allTracks,
            ) { loaded, _ ->
                when (loaded) {
                    is Loaded.Ready -> Loaded.Ready(loaded.value.copy(tracks = resonate.current(loaded.value.tracks)))
                    else -> loaded
                }
            }
            .stateIn(viewModelScope, shared, Loaded.Loading)

    val playing: StateFlow<ULong?> = resonate.playingId().stateIn(viewModelScope, shared, null)
}

@Composable
fun FavouritesScreen() {
    val model = resonateViewModel { FavouritesViewModel(it) }
    val loaded by model.tracks.collectAsStateWithLifecycle()
    val playing by model.playing.collectAsStateWithLifecycle()
    SongCollection(
        title = stringResource(R.string.library_favourites),
        overline = stringResource(R.string.library_overline_playlist),
        loaded = loaded,
        onRetry = {},
        playing = playing,
        tag = "favourites",
        empty = stringResource(R.string.library_favourites_empty),
    ) { side ->
        ShapeCover(side, MaterialShapes.Heart.toShape())
    }
}

@Composable
fun MixScreen(mix: Mix) {
    val model = resonateViewModel(key = "mix:${mix.name}") { MixViewModel(it, mix) }
    val loaded by model.page.collectAsStateWithLifecycle()
    val playing by model.playing.collectAsStateWithLifecycle()
    val covers = loaded.valueOrNull()?.covers.orEmpty()
    SongCollection(
        title = stringResource(mix.title),
        overline = stringResource(R.string.library_made_for_you),
        detail = stringResource(mix.subtitle),
        loaded = loaded.map { it.tracks },
        onRetry = model.retry::again,
        playing = playing,
        tag = "mix",
        exploreScope = ExploreScope.Filtered(mix.filter()),
        empty = stringResource(R.string.library_mix_empty),
    ) { side ->
        PlaylistCover(covers, side, shape = MaterialTheme.shapes.extraLarge)
    }
}

@Composable
fun SuggestedScreen(suggested: Screen.Suggested) {
    val resonate = LocalResonate.current
    val model =
        resonateViewModel(key = "suggested:${suggested.name}:${suggested.search}") {
            SuggestedViewModel(it, suggested.search)
        }
    val loaded by model.page.collectAsStateWithLifecycle()
    val playing by model.playing.collectAsStateWithLifecycle()
    val covers = loaded.valueOrNull()?.covers.orEmpty()
    SongCollection(
        title = suggested.name,
        overline = stringResource(R.string.library_made_for_you),
        detail = suggested.detail,
        loaded = loaded.map { it.tracks },
        onRetry = model.retry::again,
        playing = playing,
        tag = "suggested",
        exploreScope = ExploreScope.Saved(suggested.search.native()),
        empty = stringResource(R.string.library_suggested_empty),
        keep = { resonate.features.keepSearchAsPlaylist(suggested.name, suggested.search.native()) },
    ) { side ->
        PlaylistCover(covers, side, shape = MaterialTheme.shapes.extraLarge)
    }
}

private fun <T, R> Loaded<T>.map(transform: (T) -> R): Loaded<R> =
    when (this) {
        Loaded.Loading -> Loaded.Loading
        Loaded.Failed -> Loaded.Failed
        is Loaded.Ready -> Loaded.Ready(transform(value))
    }

@Composable
private fun SongCollection(
    title: String,
    overline: String,
    loaded: Loaded<List<Track>>,
    onRetry: () -> Unit,
    playing: ULong?,
    tag: String,
    empty: String,
    detail: String? = null,
    keep: (() -> Unit)? = null,
    exploreScope: ExploreScope? = null,
    cover: @Composable (androidx.compose.ui.unit.Dp) -> Unit,
) {
    val resonate = LocalResonate.current
    val sheets = LocalSheets.current
    val exploreEnabled by resonate.features.explore.collectAsStateWithLifecycle()
    val online by resonate.features.online.collectAsStateWithLifecycle()
    val jobs by resonate.features.downloads.jobs.collectAsStateWithLifecycle()
    val scope = rememberCoroutineScope()
    val downloads = remember(resonate, scope) { Downloads(resonate, scope) }
    var mixExplore by remember(exploreScope, exploreEnabled) { mutableStateOf(false) }
    var exploreRetry by remember(exploreScope) { mutableIntStateOf(0) }
    var dismissed by remember(exploreScope) { mutableStateOf(emptySet<String>()) }
    var choosing by remember(exploreScope) { mutableStateOf<SearchSong?>(null) }
    val mixFeed = rememberMixExplore(exploreScope, exploreScope != null && exploreEnabled && mixExplore, exploreRetry)
    val suggested =
        if (exploreScope != null && exploreEnabled && mixExplore) {
            mixFeed.valueOrNull()?.songs.orEmpty().filter { it.song.uri !in dismissed }
        } else {
            emptyList()
        }
    val heldDownloads = DownloadPlan.held(jobs)
    val tracks = loaded.valueOrNull().orEmpty()
    val selection = rememberSelection<ULong>()
    LaunchedEffect(tracks) { selection.retain(tracks.map { it.id }) }
    val playBlended: (List<Track>, Int, Boolean) -> Unit = { shown, startAt, shuffled ->
        if (mixExplore && suggested.isNotEmpty()) {
            scope.launch {
                val byId = shown.associateBy { it.id }
                val queue =
                    withContext(Dispatchers.IO) {
                        runCatching {
                            resonate.library.holding { native ->
                                native.weaveMixQueue(shown.map { it.id }, suggested, shuffled)
                            }
                        }.getOrNull()
                    }
                if (!queue.isNullOrEmpty()) {
                    resonate.playback.playMixed(queue, byId, if (shuffled) 0 else startAt)
                } else if (shuffled) {
                    resonate.playShuffled(shown)
                } else {
                    resonate.playInOrder(shown)
                }
            }
        } else if (shuffled) {
            resonate.playShuffled(shown)
        } else {
            resonate.playback.play(shown, startAt)
        }
    }
    CollectionPage(
        title,
        loaded,
        onRetry,
        tag = tag,
        selectionBar =
            if (!selection.active) null
            else {
                {
                    SongSelectionBar(
                        tracks = selection.picked(tracks) { it.id },
                        onSelectAll = { selection.selectAll(tracks.map { it.id }) },
                        onClose = selection::clear,
                    )
                }
            },
        actions = {
            OverflowMenu(
                listOf(
                    listOf(
                        MenuEntry.Action(
                            stringResource(R.string.library_play_next),
                            Icons.AutoMirrored.Rounded.PlaylistPlay,
                        ) {
                            resonate.playNext(tracks)
                        },
                        MenuEntry.Action(
                            stringResource(R.string.library_add_to_queue),
                            Icons.AutoMirrored.Rounded.QueueMusic,
                        ) {
                            resonate.addToQueue(tracks)
                        },
                        MenuEntry.Action(
                            stringResource(R.string.library_add_to_playlist),
                            Icons.AutoMirrored.Rounded.PlaylistAdd,
                        ) {
                            sheets.addToPlaylist(tracks)
                        },
                    )
                ) +
                    listOfNotNull(
                        keep?.let { keep ->
                            listOf(
                                MenuEntry.Action(
                                    stringResource(R.string.library_keep_as_playlist),
                                    Icons.AutoMirrored.Rounded.PlaylistAdd,
                                    run = keep,
                                )
                            )
                        }
                    ),
                description = stringResource(R.string.library_options_for, title),
            )
        },
    ) { shown ->
        item("header") {
            CollectionHeader(
                title = title,
                overline = overline,
                summary = collectionSummary(words(), tracks = shown),
                extra =
                    detail?.let { text ->
                        { align ->
                            Text(
                                text,
                                style = MaterialTheme.typography.bodyMedium,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                                textAlign = align,
                                modifier = Modifier.fillMaxWidth(),
                            )
                        }
                    },
                onPlay = { playBlended(shown, 0, false) },
                onShuffle = { playBlended(shown, 0, true) },
                playable = shown.isNotEmpty(),
                cover = cover,
            )
        }
        if (exploreScope != null && exploreEnabled) {
            item("explore-toggle") {
                ExploreMixToggle(enabled = mixExplore, onToggle = { mixExplore = !mixExplore })
            }
            if (mixExplore) {
                item("explore-downloads") { DownloadCard(downloads) }
                val note =
                    when {
                        !online -> R.string.explore_offline
                        mixFeed is Loaded.Loading -> R.string.explore_loading
                        mixFeed is Loaded.Failed -> R.string.explore_failed
                        mixFeed.valueOrNull()?.services == 0u -> R.string.explore_services
                        suggested.isEmpty() -> R.string.explore_empty
                        else -> null
                    }
                if (note != null) {
                    item("explore-note") {
                        Text(
                            stringResource(note),
                            modifier = Modifier.padding(horizontal = 16.dp, vertical = 4.dp),
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                        if (mixFeed is Loaded.Failed) {
                            TextButton(onClick = { exploreRetry++ }) {
                                Text(stringResource(R.string.explore_retry))
                            }
                        }
                    }
                }
            }
        }
        if (shown.isEmpty()) item("empty") { EmptyCollection(detail = empty) }
        val entries =
            if (mixExplore && suggested.isNotEmpty()) weaveAndroidMix(shown, suggested)
            else shown.mapIndexed { index, track -> MixListEntry.Local(track, index) }
        entries.forEachIndexed { index, entry ->
            when (entry) {
                is MixListEntry.Local -> {
                    val track = entry.track
                    item("row:$index:${track.id}") {
                        TrackRow(
                            track,
                            onClick = { playBlended(shown, index, false) },
                            playing = track.id == playing,
                            onMore = { sheets.song(track) },
                            selection = selection.row(track.id),
                        )
                    }
                }
                is MixListEntry.Suggested -> {
                    val offer = entry.song
                    item("suggested:$index:${offer.song.uri}") {
                        ExploreWovenRow(
                            offer = offer,
                            queued = offer.download.heldKey() in heldDownloads,
                            onPlay = { playBlended(shown, index, false) },
                            onDownload = {
                                downloads.request(listOf(offer.download)) { choosing = offer.download }
                            },
                            onDismiss = { dismissed = dismissed + offer.song.uri },
                        )
                    }
                }
            }
        }
    }
    choosing?.let { song -> DownloadChooser(song, downloads) { choosing = null } }
}
