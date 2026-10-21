package org.resonate.android.ui.library

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.scaleIn
import androidx.compose.animation.scaleOut
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.calculateEndPadding
import androidx.compose.foundation.layout.calculateStartPadding
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.pager.HorizontalPager
import androidx.compose.foundation.pager.rememberPagerState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Add
import androidx.compose.material.icons.rounded.CreateNewFolder
import androidx.compose.material.icons.rounded.Download
import androidx.compose.material.icons.rounded.Handyman
import androidx.compose.material.icons.rounded.Refresh
import androidx.compose.material3.ExtendedFloatingActionButton
import androidx.compose.material3.Icon
import androidx.compose.material3.PrimaryScrollableTabRow
import androidx.compose.material3.PrimaryTabRow
import androidx.compose.material3.Tab
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import kotlinx.coroutines.launch
import org.resonate.android.ui.LocalResonate
import org.resonate.android.ui.components.LocalSheets
import org.resonate.android.ui.components.Page
import org.resonate.android.ui.components.SettingsAction
import org.resonate.android.ui.components.TitleSize
import org.resonate.android.ui.navigation.LocalNavigator
import org.resonate.android.ui.navigation.Screen
import org.resonate.android.ui.resonateViewModel

private enum class Shelf(val title: String) {
    SONGS("Songs"),
    ALBUMS("Albums"),
    ARTISTS("Artists"),
    PLAYLISTS("Playlists"),
}

@Composable
fun LibraryScreen() {
    val model = resonateViewModel { LibraryViewModel(it) }
    val resonate = LocalResonate.current
    val navigator = LocalNavigator.current
    val sheets = LocalSheets.current
    val scope = rememberCoroutineScope()
    val pager = rememberPagerState { Shelf.entries.size }
    val ready by model.ready.collectAsStateWithLifecycle()
    val empty by model.catalogEmpty.collectAsStateWithLifecycle()
    val onPlaylists = pager.currentPage == Shelf.PLAYLISTS.ordinal
    val newPlaylist = { sheets.name("New playlist") { model.createPlaylist(it) } }
    Page(
        "Library",
        size = TitleSize.LARGE,
        back = false,
        actions = {
            SettingsAction()
            OverflowMenu(
                listOf(
                    listOf(
                        MenuEntry.Action("Add music folder", Icons.Rounded.CreateNewFolder) {
                            resonate.host.chooseFolder()
                        },
                        MenuEntry.Action("Use Downloads", Icons.Rounded.Download) {
                            resonate.host.useDownloads()
                        },
                        MenuEntry.Action("Rescan library", Icons.Rounded.Refresh) { model.rescan() },
                    ),
                    listOf(
                        MenuEntry.Action("Library tools", Icons.Rounded.Handyman) {
                            navigator.open(Screen.LibraryTools)
                        }
                    ),
                )
            )
        },
        floatingActionButton = {
            AnimatedVisibility(
                visible = onPlaylists && !empty,
                enter = scaleIn(),
                exit = scaleOut(),
            ) {
                ExtendedFloatingActionButton(
                    onClick = newPlaylist,
                    icon = { Icon(Icons.Rounded.Add, contentDescription = null) },
                    text = { Text("New playlist") },
                    modifier = Modifier.testTag("new-playlist"),
                )
            }
        },
    ) { padding ->
        val direction = LocalLayoutDirection.current
        val below =
            PaddingValues(
                start = padding.calculateStartPadding(direction),
                end = padding.calculateEndPadding(direction),
                bottom = padding.calculateBottomPadding(),
            )
        when {
            !ready -> Loading(Modifier.padding(padding))
            empty -> Box(Modifier.fillMaxSize().padding(padding)) { Welcome() }
            else ->
                Column(Modifier.fillMaxSize().padding(top = padding.calculateTopPadding())) {
                    val tabs: @Composable () -> Unit = {
                        Shelf.entries.forEach { shelf ->
                            Tab(
                                selected = pager.currentPage == shelf.ordinal,
                                onClick = { scope.launch { pager.animateScrollToPage(shelf.ordinal) } },
                                text = { Text(shelf.title, maxLines = 1) },
                                modifier = Modifier.testTag("shelf-${shelf.name.lowercase()}"),
                            )
                        }
                    }
                    if (LocalDensity.current.fontScale > LARGE_TEXT) {
                        PrimaryScrollableTabRow(selectedTabIndex = pager.currentPage, tabs = tabs)
                    } else {
                        PrimaryTabRow(selectedTabIndex = pager.currentPage, tabs = tabs)
                    }
                    HorizontalPager(
                        state = pager,
                        modifier = Modifier.fillMaxSize(),
                        beyondViewportPageCount = 1,
                        key = { Shelf.entries[it].name },
                    ) { page ->
                        when (Shelf.entries[page]) {
                            Shelf.SONGS -> SongsTab(model, below)
                            Shelf.ALBUMS -> AlbumsTab(model, below)
                            Shelf.ARTISTS -> ArtistsTab(model, below)
                            Shelf.PLAYLISTS ->
                                PlaylistsTab(
                                    model,
                                    PaddingValues(
                                        start = below.calculateStartPadding(direction),
                                        end = below.calculateEndPadding(direction),
                                        bottom = below.calculateBottomPadding() + 88.dp,
                                    ),
                                )
                        }
                    }
                }
        }
    }
}

const val LARGE_TEXT = 1.3f
