package org.resonate.android.ui.library

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Download
import androidx.compose.material.icons.rounded.PlayArrow
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
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
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import java.time.ZonedDateTime
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.resonate.android.DownloadPlan
import org.resonate.android.R
import org.resonate.android.heldKey
import org.resonate.android.nativecore.ExploreFeed
import org.resonate.android.nativecore.ExploreScope
import org.resonate.android.nativecore.SearchSong
import org.resonate.android.ui.LocalResonate
import org.resonate.android.ui.search.DownloadCard
import org.resonate.android.ui.search.DownloadChooser
import org.resonate.android.ui.search.Downloads

@Composable
internal fun ExploreTabs(exploring: Boolean, change: (Boolean) -> Unit) {
    SingleChoiceSegmentedButtonRow(
        Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp)
    ) {
        listOf(R.string.explore_library, R.string.explore_title).forEachIndexed { index, label ->
            val explore = index == 1
            SegmentedButton(
                selected = exploring == explore,
                onClick = { change(explore) },
                shape = SegmentedButtonDefaults.itemShape(index, 2),
            ) {
                Text(stringResource(label))
            }
        }
    }
}

@Composable
internal fun ExploreSongs(scope: ExploreScope) {
    val resonate = LocalResonate.current
    val features = resonate.features
    val online by features.online.collectAsStateWithLifecycle()
    val accounts by features.served.accounts.collectAsStateWithLifecycle()
    val snapshot by resonate.library.snapshot.collectAsStateWithLifecycle()
    val jobs by features.downloads.jobs.collectAsStateWithLifecycle()
    val coroutineScope = rememberCoroutineScope()
    val downloads = remember(resonate, coroutineScope) { Downloads(resonate, coroutineScope) }
    var choosing by remember(scope, accounts, online) { mutableStateOf<SearchSong?>(null) }
    val now = ZonedDateTime.now()
    val day = now.toLocalDate()
    var retry by remember(scope, accounts, day, online) { mutableIntStateOf(0) }
    var retried by remember(scope, accounts, day, online) { mutableIntStateOf(0) }
    var page by
        remember(scope, accounts, day, online) {
            mutableStateOf<Loaded<ExploreFeed>>(Loaded.Loading)
        }
    val catalog = snapshot?.catalogRevision
    LaunchedEffect(scope, accounts, day, online, retry, catalog) {
        if (!online) return@LaunchedEffect
        val refresh = retry != retried
        retried = retry
        if (refresh) page = Loaded.Loading
        page = try {
            val found = withContext(Dispatchers.IO) {
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
    val retrySearch: () -> Unit = { retry++ }
    val feed = page.valueOrNull()
    val message = when {
        !online -> R.string.explore_offline
        page is Loaded.Loading -> R.string.explore_loading
        page is Loaded.Failed -> R.string.explore_failed
        feed?.services == 0u -> R.string.explore_services
        feed != null && feed.failed > 0u -> R.string.explore_failed
        feed?.songs.isNullOrEmpty() -> R.string.explore_empty
        else -> null
    }
    Column(Modifier.fillMaxWidth()) {
        DownloadCard(downloads)
        if (online && feed != null) {
            val held = DownloadPlan.held(jobs)
            feed.songs.forEach { offer ->
                val song = offer.song
                Row(
                    Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 4.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Column(Modifier.weight(1f)) {
                        Text(song.title, maxLines = 2, overflow = TextOverflow.Ellipsis)
                        song.artist?.let { Text(it, style = MaterialTheme.typography.bodySmall) }
                        Text(offer.service, style = MaterialTheme.typography.labelSmall)
                    }
                    IconButton(onClick = { resonate.playback.playServed(listOf(song.uri)) }) {
                        Icon(
                            Icons.Rounded.PlayArrow,
                            stringResource(R.string.explore_play, song.title),
                        )
                    }
                    IconButton(
                        enabled = offer.download.heldKey() !in held,
                        onClick = { downloads.request(listOf(offer.download)) { choosing = offer.download } },
                    ) {
                        Icon(
                            Icons.Rounded.Download,
                            stringResource(R.string.explore_download, song.title),
                        )
                    }
                }
            }
        }
        if (message != null) {
            Text(
                stringResource(message),
                modifier = Modifier.padding(12.dp),
                style = MaterialTheme.typography.bodyMedium,
            )
            if (page is Loaded.Failed || (feed?.failed ?: 0u) > 0u) {
                TextButton(onClick = retrySearch) { Text(stringResource(R.string.explore_retry)) }
            }
        }
    }
    choosing?.let { song -> DownloadChooser(song, downloads) { choosing = null } }
}
