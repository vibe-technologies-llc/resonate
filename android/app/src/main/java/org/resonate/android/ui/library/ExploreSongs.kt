package org.resonate.android.ui.library

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Album
import androidx.compose.material.icons.rounded.AutoAwesome
import androidx.compose.material.icons.rounded.Category
import androidx.compose.material.icons.rounded.Check
import androidx.compose.material.icons.rounded.Close
import androidx.compose.material.icons.rounded.Download
import androidx.compose.material.icons.rounded.Favorite
import androidx.compose.material.icons.rounded.Person
import androidx.compose.material.icons.rounded.PlayArrow
import androidx.compose.material.icons.rounded.Refresh
import androidx.compose.material.icons.rounded.Shuffle
import androidx.compose.material3.Button
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import java.time.ZonedDateTime
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.resonate.android.DownloadPlan
import org.resonate.android.R
import org.resonate.android.heldKey
import org.resonate.android.nativecore.ExploreFeed
import org.resonate.android.nativecore.ExplorePlaylist
import org.resonate.android.nativecore.ExplorePlaylistKind
import org.resonate.android.nativecore.ExplorePlaylistsFeed
import org.resonate.android.nativecore.ExploreScope
import org.resonate.android.nativecore.ExploreSong
import org.resonate.android.nativecore.SearchSong
import org.resonate.android.nativecore.Track
import org.resonate.android.ui.LocalResonate
import org.resonate.android.ui.components.Page
import org.resonate.android.ui.navigation.Screen
import org.resonate.android.ui.search.DownloadCard
import org.resonate.android.ui.search.DownloadChooser
import org.resonate.android.ui.search.Downloads

internal sealed interface MixListEntry {
    data class Local(val track: Track, val localIndex: Int) : MixListEntry

    data class Suggested(val song: ExploreSong) : MixListEntry
}

internal fun weaveAndroidMix(
    tracks: List<Track>,
    suggestions: List<ExploreSong>,
): List<MixListEntry> {
    if (suggestions.isEmpty()) return tracks.mapIndexed { index, track -> MixListEntry.Local(track, index) }
    if (tracks.isEmpty()) return suggestions.map { MixListEntry.Suggested(it) }
    val cadence =
        when {
            tracks.size >= 7 -> 3
            tracks.size >= 4 -> 2
            else -> 1
        }
    val remaining = ArrayDeque(suggestions)
    val woven = ArrayList<MixListEntry>(tracks.size + suggestions.size)
    tracks.forEachIndexed { index, track ->
        woven.add(MixListEntry.Local(track, index))
        if ((index + 1) % cadence == 0 && remaining.isNotEmpty()) {
            val prevArtist = track.artist.trim().lowercase()
            val nextArtist = tracks.getOrNull(index + 1)?.artist?.trim()?.lowercase()
            val ideal =
                remaining.indexOfFirst { offer ->
                    val artist = offer.song.artist?.trim()?.lowercase().orEmpty()
                    artist != prevArtist && artist != nextArtist
                }
            val pick = if (ideal >= 0) ideal else 0
            woven.add(MixListEntry.Suggested(remaining.removeAt(pick)))
        }
    }
    return woven
}

internal val ExplorePlaylistKind.icon: ImageVector
    get() =
        when (this) {
            ExplorePlaylistKind.DISCOVER_MIX -> Icons.Rounded.AutoAwesome
            ExplorePlaylistKind.BEYOND_ARTIST -> Icons.Rounded.Person
            ExplorePlaylistKind.GENRE_DISCOVERY -> Icons.Rounded.Category
            ExplorePlaylistKind.FAVOURITES_EXPANSION -> Icons.Rounded.Favorite
            ExplorePlaylistKind.REDISCOVER_BEYOND -> Icons.Rounded.Album
        }

@Composable
internal fun rememberExplorePlaylists(
    retry: Int,
): Loaded<ExplorePlaylistsFeed> {
    val resonate = LocalResonate.current
    val features = resonate.features
    val online by features.online.collectAsStateWithLifecycle()
    val accounts by features.served.accounts.collectAsStateWithLifecycle()
    val snapshot by resonate.library.snapshot.collectAsStateWithLifecycle()
    val now = ZonedDateTime.now()
    val day = now.toLocalDate()
    var retried by remember(accounts, day, online) { mutableIntStateOf(0) }
    var page by remember(accounts, day, online) { mutableStateOf<Loaded<ExplorePlaylistsFeed>>(Loaded.Loading) }
    val catalog = snapshot?.catalogRevision
    LaunchedEffect(accounts, day, online, retry, catalog) {
        if (!online) return@LaunchedEffect
        val refresh = retry != retried
        retried = retry
        if (refresh) page = Loaded.Loading
        page =
            try {
                val found =
                    withContext(Dispatchers.IO) {
                        resonate.library.holding { native ->
                            native.serveFrom(accounts)
                            native.explorePlaylists(now.zone.id, now.offset.totalSeconds, refresh)
                        }
                    }
                Loaded.Ready(found)
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (_: Exception) {
                Loaded.Failed
            }
    }
    return page
}

@Composable
internal fun rememberMixExplore(
    scope: ExploreScope?,
    enabled: Boolean,
    retry: Int,
): Loaded<ExploreFeed> {
    val resonate = LocalResonate.current
    val features = resonate.features
    val online by features.online.collectAsStateWithLifecycle()
    val accounts by features.served.accounts.collectAsStateWithLifecycle()
    val snapshot by resonate.library.snapshot.collectAsStateWithLifecycle()
    val now = ZonedDateTime.now()
    val day = now.toLocalDate()
    var retried by remember(scope, accounts, day, online) { mutableIntStateOf(0) }
    var page by remember(scope, accounts, day, online) { mutableStateOf<Loaded<ExploreFeed>>(Loaded.Loading) }
    val catalog = snapshot?.catalogRevision
    LaunchedEffect(scope, enabled, accounts, day, online, retry, catalog) {
        if (!enabled || scope == null || !online) return@LaunchedEffect
        val refresh = retry != retried
        retried = retry
        if (refresh) page = Loaded.Loading
        page =
            try {
                val found =
                    withContext(Dispatchers.IO) {
                        resonate.library.holding { native ->
                            native.serveFrom(accounts)
                            native.explore(scope, now.zone.id, now.offset.totalSeconds, refresh)
                        }
                    }
                Loaded.Ready(found)
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (_: Exception) {
                Loaded.Failed
            }
    }
    return page
}

@Composable
internal fun ExploreMixToggle(enabled: Boolean, onToggle: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp),
        horizontalArrangement = Arrangement.Start,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        FilterChip(
            selected = enabled,
            onClick = onToggle,
            label = { Text(stringResource(R.string.explore_mix_mode)) },
            leadingIcon = {
                Icon(
                    Icons.Rounded.AutoAwesome,
                    contentDescription = null,
                    modifier = Modifier.size(18.dp),
                )
            },
        )
    }
}

@Composable
internal fun ExploreWovenRow(
    offer: ExploreSong,
    queued: Boolean,
    onPlay: () -> Unit,
    onDownload: () -> Unit,
    onDismiss: () -> Unit,
) {
    val song = offer.song
    Row(
        Modifier.fillMaxWidth()
            .clickable(onClick = onPlay)
            .padding(horizontal = 16.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Surface(
            color = MaterialTheme.colorScheme.secondaryContainer,
            contentColor = MaterialTheme.colorScheme.onSecondaryContainer,
            shape = MaterialTheme.shapes.medium,
            modifier = Modifier.size(44.dp),
        ) {
            Box(contentAlignment = Alignment.Center) {
                Icon(Icons.Rounded.AutoAwesome, contentDescription = null, modifier = Modifier.size(20.dp))
            }
        }
        Column(Modifier.weight(1f).padding(horizontal = 12.dp)) {
            Text(
                song.title,
                style = MaterialTheme.typography.bodyLarge,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            song.artist?.let {
                Text(
                    it,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            Text(
                stringResource(R.string.explore_suggested_badge, offer.service),
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.primary,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
        IconButton(enabled = !queued, onClick = onDownload) {
            Icon(
                if (queued) Icons.Rounded.Check else Icons.Rounded.Download,
                contentDescription =
                    stringResource(
                        if (queued) R.string.explore_downloaded else R.string.explore_download,
                        song.title,
                    ),
            )
        }
        IconButton(onClick = onDismiss) {
            Icon(
                Icons.Rounded.Close,
                contentDescription = stringResource(R.string.explore_dismiss, song.title),
            )
        }
    }
}

@Composable
internal fun ExploreDownloadButton(uri: String) {
    val resonate = LocalResonate.current
    val features = resonate.features
    val jobs by features.downloads.jobs.collectAsStateWithLifecycle()
    val coroutineScope = rememberCoroutineScope()
    val downloads = remember(resonate, coroutineScope) { Downloads(resonate, coroutineScope) }
    var offer by remember(uri) { mutableStateOf<ExploreSong?>(null) }
    var choosing by remember(uri) { mutableStateOf<SearchSong?>(null) }
    LaunchedEffect(uri) {
        offer =
            withContext(Dispatchers.IO) {
                runCatching { resonate.library.holding { it.exploreDownloadFor(uri) } }.getOrNull()
            }
    }
    val current = offer ?: return
    val held = DownloadPlan.held(jobs)
    val queued = current.download.heldKey() in held
    IconButton(
        enabled = !queued,
        onClick = { downloads.request(listOf(current.download)) { choosing = current.download } },
    ) {
        Icon(
            if (queued) Icons.Rounded.Check else Icons.Rounded.Download,
            contentDescription =
                stringResource(
                    if (queued) R.string.explore_downloaded else R.string.explore_download,
                    current.song.title,
                ),
            tint =
                if (queued) MaterialTheme.colorScheme.primary
                else MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
    choosing?.let { songs -> DownloadChooser(songs, downloads) { choosing = null } }
}

@Composable
internal fun MixIntoQueueButton() {
    val resonate = LocalResonate.current
    val exploreEnabled by resonate.features.explore.collectAsStateWithLifecycle()
    val online by resonate.features.online.collectAsStateWithLifecycle()
    val accounts by resonate.features.served.accounts.collectAsStateWithLifecycle()
    val scope = rememberCoroutineScope()
    var working by remember { mutableStateOf(false) }
    if (!exploreEnabled || !online) return
    IconButton(
        enabled = !working,
        onClick = {
            working = true
            scope.launch {
                val now = ZonedDateTime.now()
                withContext(Dispatchers.IO) {
                    runCatching {
                        resonate.library.holding { native ->
                            native.serveFrom(accounts)
                            native.mixSuggestionsIntoQueue(now.zone.id, now.offset.totalSeconds)
                        }
                    }
                }
                working = false
            }
        },
    ) {
        Icon(Icons.Rounded.AutoAwesome, stringResource(R.string.explore_mix_queue))
    }
}

@Composable
fun ExplorePlaylistScreen(screen: Screen.ExplorePlaylist) {
    val resonate = LocalResonate.current
    val features = resonate.features
    val jobs by features.downloads.jobs.collectAsStateWithLifecycle()
    val coroutineScope = rememberCoroutineScope()
    val downloads = remember(resonate, coroutineScope) { Downloads(resonate, coroutineScope) }
    var retry by remember(screen.id) { mutableIntStateOf(0) }
    var choosing by remember(screen.id) { mutableStateOf<List<SearchSong>?>(null) }
    val loaded = rememberExplorePlaylists(retry)
    val playlist: ExplorePlaylist? =
        loaded.valueOrNull()?.playlists?.firstOrNull { it.id == screen.id }
    val held = DownloadPlan.held(jobs)

    Page(playlist?.name ?: screen.name) { padding ->
        LazyColumn(
            modifier = Modifier.fillMaxSize(),
            contentPadding =
                PaddingValues(
                    top = padding.calculateTopPadding() + 8.dp,
                    bottom = padding.calculateBottomPadding() + 24.dp,
                ),
        ) {
            item("downloads") { DownloadCard(downloads) }
            if (loaded is Loaded.Loading && playlist == null) {
                item("loading") {
                    Text(
                        stringResource(R.string.explore_loading),
                        modifier = Modifier.padding(24.dp),
                        style = MaterialTheme.typography.bodyMedium,
                    )
                }
            } else if (playlist == null) {
                item("gone") {
                    Column(Modifier.padding(24.dp)) {
                        Text(
                            stringResource(R.string.explore_playlist_gone),
                            style = MaterialTheme.typography.bodyMedium,
                        )
                        TextButton(onClick = { retry++ }) {
                            Text(stringResource(R.string.explore_retry))
                        }
                    }
                }
            } else {
                item("header") {
                    Column(Modifier.fillMaxWidth().padding(horizontal = 20.dp, vertical = 12.dp)) {
                        Text(
                            stringResource(R.string.explore_playlist_overline),
                            style = MaterialTheme.typography.labelMedium,
                            color = MaterialTheme.colorScheme.primary,
                        )
                        Spacer(Modifier.height(4.dp))
                        Text(playlist.name, style = MaterialTheme.typography.headlineSmall)
                        Spacer(Modifier.height(4.dp))
                        Text(
                            playlist.detail,
                            style = MaterialTheme.typography.bodyMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                        Spacer(Modifier.height(4.dp))
                        Text(
                            pluralStringResource(
                                R.plurals.count_songs,
                                playlist.songs.size,
                                playlist.songs.size,
                            ),
                            style = MaterialTheme.typography.labelMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                        Spacer(Modifier.height(12.dp))
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            Button(
                                onClick = {
                                    resonate.playback.playServed(playlist.songs.map { it.song.uri }, 0)
                                }
                            ) {
                                Icon(Icons.Rounded.PlayArrow, null)
                                Text(stringResource(R.string.collection_play))
                            }
                            OutlinedButton(
                                onClick = {
                                    val shuffled = playlist.songs.map { it.song.uri }.shuffled()
                                    resonate.playback.playServed(shuffled, 0)
                                }
                            ) {
                                Icon(Icons.Rounded.Shuffle, null)
                                Text(stringResource(R.string.collection_shuffle))
                            }
                            IconButton(
                                onClick = {
                                    val pending = playlist.songs.map { it.download }
                                    if (pending.isNotEmpty()) {
                                        downloads.request(pending) { choosing = pending }
                                    }
                                }
                            ) {
                                Icon(
                                    Icons.Rounded.Download,
                                    stringResource(R.string.explore_download_all),
                                )
                            }
                            IconButton(onClick = { retry++ }) {
                                Icon(Icons.Rounded.Refresh, stringResource(R.string.explore_refresh))
                            }
                        }
                    }
                }
                itemsIndexed(playlist.songs, key = { _, offer -> offer.song.uri }) { index, offer ->
                    val song = offer.song
                    val queued = offer.download.heldKey() in held
                    Row(
                        Modifier.fillMaxWidth()
                            .clickable {
                                resonate.playback.playServed(
                                    playlist.songs.map { it.song.uri },
                                    index,
                                )
                            }
                            .padding(horizontal = 16.dp, vertical = 10.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Column(Modifier.weight(1f)) {
                            Text(song.title, maxLines = 1, overflow = TextOverflow.Ellipsis)
                            song.artist?.let {
                                Text(
                                    it,
                                    style = MaterialTheme.typography.bodySmall,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    maxLines = 1,
                                    overflow = TextOverflow.Ellipsis,
                                )
                            }
                            Text(
                                offer.service,
                                style = MaterialTheme.typography.labelSmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                        IconButton(
                            enabled = !queued,
                            onClick = {
                                downloads.request(listOf(offer.download)) { choosing = listOf(offer.download) }
                            },
                        ) {
                            Icon(
                                if (queued) Icons.Rounded.Check else Icons.Rounded.Download,
                                stringResource(
                                    if (queued) R.string.explore_downloaded else R.string.explore_download,
                                    song.title,
                                ),
                            )
                        }
                    }
                }
            }
        }
    }
    choosing?.let { songs -> DownloadChooser(songs, downloads) { choosing = null } }
}
