package org.resonate.android.ui.home

import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Add
import androidx.compose.material.icons.rounded.Groups
import androidx.compose.material.icons.rounded.SettingsRemote
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.material3.pulltorefresh.PullToRefreshDefaults
import androidx.compose.material3.pulltorefresh.rememberPullToRefreshState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LifecycleResumeEffect
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.repeatOnLifecycle
import java.time.LocalTime
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.drop
import kotlinx.coroutines.flow.filter
import org.resonate.android.R
import org.resonate.android.ui.LocalResonate
import org.resonate.android.ui.components.BrowseShortcuts
import org.resonate.android.ui.components.CollectionRow
import org.resonate.android.ui.components.HandoffRow
import org.resonate.android.ui.components.IconTile
import org.resonate.android.ui.components.LocalSheets
import org.resonate.android.ui.components.Page
import org.resonate.android.ui.components.SectionHeader
import org.resonate.android.ui.components.SettingsAction
import org.resonate.android.ui.components.TitleSize
import org.resonate.android.ui.library.LibraryOpening
import org.resonate.android.ui.library.Loading
import org.resonate.android.ui.library.PlaylistCard
import org.resonate.android.ui.library.Shelf
import org.resonate.android.ui.library.Shelves
import org.resonate.android.ui.library.Unreadable
import org.resonate.android.ui.library.WelcomeWords
import org.resonate.android.ui.library.greeting
import org.resonate.android.ui.library.screen
import org.resonate.android.ui.library.showLibrary
import org.resonate.android.ui.navigation.LocalNavigator
import org.resonate.android.ui.navigation.Screen
import org.resonate.android.ui.player.LocalPlayerExpansion
import org.resonate.android.ui.resonateViewModel
import org.resonate.android.ui.settings.deviceState
import org.resonate.android.ui.settings.iconFor
import org.resonate.android.words

@Composable
fun HomeScreen() {
    val model = resonateViewModel { HomeViewModel(it) }
    val ready by model.ready.collectAsStateWithLifecycle()
    val songs by model.songs.collectAsStateWithLifecycle()
    val unread by model.unread.collectAsStateWithLifecycle()
    var hour by remember { mutableIntStateOf(LocalTime.now().hour) }
    FollowTheFeed(model)
    LifecycleResumeEffect(model) {
        hour = LocalTime.now().hour
        model.cameForward()
        onPauseOrDispose {}
    }
    Page(greeting(words(), hour), size = TitleSize.LARGE, back = false, actions = { SettingsAction() }) { padding ->
        when {
            !ready -> Loading(Modifier.padding(padding))
            songs.isEmpty() && unread -> Unreadable(model::reread, Modifier.padding(padding))
            else -> HomeFeedList(model, empty = songs.isEmpty(), padding = padding)
        }
    }
}

@Composable
private fun FollowTheFeed(model: HomeViewModel) {
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    val expansion = LocalPlayerExpansion.current
    val navigator = LocalNavigator.current
    LaunchedEffect(model, lifecycle) {
        lifecycle.repeatOnLifecycle(Lifecycle.State.STARTED) { model.followCatalog() }
    }
    LaunchedEffect(model, expansion) {
        snapshotFlow { expansion.shown }
            .distinctUntilChanged()
            .drop(1)
            .filter { shown -> !shown && navigator.atHome }
            .collect { model.feed.ask() }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun HomeFeedList(model: HomeViewModel, empty: Boolean, padding: PaddingValues) {
    val refreshing by model.refreshing.collectAsStateWithLifecycle()
    val pulled = rememberPullToRefreshState()
    PullToRefreshBox(
        isRefreshing = refreshing,
        onRefresh = model::pull,
        state = pulled,
        modifier = Modifier.fillMaxSize(),
        indicator = {
            PullToRefreshDefaults.Indicator(
                state = pulled,
                isRefreshing = refreshing,
                modifier = Modifier.align(Alignment.TopCenter).padding(top = padding.calculateTopPadding()),
            )
        },
    ) {
        val navigator = LocalNavigator.current
        val features = LocalResonate.current.features
        val sheets = LocalSheets.current
        val playlists by model.playlists.collectAsStateWithLifecycle()
        val devices by model.devices.collectAsStateWithLifecycle()
        val handoffs by model.handoffs.collectAsStateWithLifecycle()
        val feed = rememberCatalogSections(model)
        val newPlaylist = stringResource(R.string.library_new_playlist)
        val seeAll = stringResource(R.string.search_see_all)
        LazyColumn(contentPadding = padding, modifier = Modifier.fillMaxSize().testTag("home")) {
            item("browse") { BrowseShortcuts(navigator::open) }
            if (empty) {
                item("welcome") { WelcomeWords(Modifier.fillMaxWidth()) }
            } else {
                catalogSections(feed, model, navigator, features, seeAll)
            }
            if (playlists.isNotEmpty() || !empty) {
                item("playlists-title") {
                    SectionHeader(
                        stringResource(R.string.library_your_playlists),
                        action = seeAll.takeIf { playlists.isNotEmpty() },
                        onAction = { navigator.showLibrary(features, LibraryOpening.playlists) },
                    )
                }
                if (playlists.isEmpty()) {
                    item("playlists-none") {
                        CollectionRow(
                            title = newPlaylist,
                            supporting = stringResource(R.string.library_playlist_invitation),
                            onClick = { sheets.name(newPlaylist) { model.createPlaylist(it) } },
                            leading = { IconTile(Icons.Rounded.Add) },
                            modifier = Modifier.testTag("home-new-playlist"),
                        )
                    }
                } else {
                    item("playlists") {
                        Shelf(playlists.take(HomeFeed.SHELVED), key = { it.id.toString() }, Modifier.testTag("home-playlists")) {
                            playlist ->
                            PlaylistCard(
                                playlist,
                                onClick = { navigator.open(playlist.screen) },
                                modifier = Modifier.width(Shelves.playlistWidth),
                            )
                        }
                    }
                }
            }
            if (!empty) {
                item("listening-title") {
                    SectionHeader(
                        stringResource(R.string.home_your_listening),
                        action = seeAll,
                        onAction = { navigator.open(Screen.Statistics) },
                    )
                }
                item("listening") {
                    ListeningGlanceCard(feed.heard, feed.glanceDay, feed.wide) { navigator.open(Screen.Statistics) }
                }
            }
            item("devices-title") { SectionHeader(stringResource(R.string.home_devices)) }
            devices.forEach { device ->
                item("device-${device.id}") {
                    val words = words()
                    CollectionRow(
                        title = device.name.ifBlank { stringResource(R.string.sync_unnamed_device) },
                        supporting = deviceState(words, device),
                        onClick = { navigator.open(Screen.SyncDevice(device.id)) },
                        leading = { IconTile(iconFor(device)) },
                        modifier = Modifier.testTag("home-device"),
                    )
                }
            }
            item("remote") {
                CollectionRow(
                    title = stringResource(R.string.home_remote),
                    supporting = stringResource(R.string.home_remote_detail),
                    onClick = { navigator.open(Screen.Sync) },
                    leading = { IconTile(Icons.Rounded.SettingsRemote) },
                    modifier = Modifier.testTag("home-remote"),
                )
            }
            handoffs.forEach { handoff ->
                item("handoff-${handoff.id}") { HandoffRow(handoff, onClick = { model.continueFrom(handoff) }) }
            }
            item("together-title") { SectionHeader(stringResource(R.string.home_together)) }
            item("party") {
                CollectionRow(
                    title = stringResource(R.string.library_listening_party),
                    supporting = stringResource(R.string.home_party_detail),
                    onClick = { navigator.open(Screen.Party) },
                    leading = { IconTile(Icons.Rounded.Groups) },
                    modifier = Modifier.testTag("home-party"),
                )
            }
        }
    }
}
