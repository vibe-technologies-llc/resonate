package org.resonate.android.ui

import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInHorizontally
import androidx.compose.animation.slideOutHorizontally
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.WindowInsetsSides
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.only
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Home
import androidx.compose.material.icons.outlined.LibraryMusic
import androidx.compose.material.icons.outlined.Search
import androidx.compose.material.icons.rounded.Home
import androidx.compose.material.icons.rounded.LibraryMusic
import androidx.compose.material.icons.rounded.Search
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.material3.adaptive.currentWindowAdaptiveInfo
import androidx.compose.material3.adaptive.navigationsuite.NavigationSuiteItem
import androidx.compose.material3.adaptive.navigationsuite.NavigationSuiteScaffold
import androidx.compose.material3.adaptive.navigationsuite.NavigationSuiteScaffoldDefaults
import androidx.compose.material3.adaptive.navigationsuite.NavigationSuiteType
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.testTagsAsResourceId
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.navigation3.ui.NavDisplay
import org.resonate.android.ui.components.ArtworkClock
import org.resonate.android.ui.components.Banners
import org.resonate.android.ui.components.FollowArtworkChanges
import org.resonate.android.ui.components.LocalArtworkClock
import org.resonate.android.ui.components.LocalSheets
import org.resonate.android.ui.components.SheetHost
import org.resonate.android.ui.components.Sheets
import org.resonate.android.ui.navigation.LocalNavigator
import org.resonate.android.ui.navigation.Screen
import org.resonate.android.ui.navigation.ScreenContent
import org.resonate.android.ui.navigation.rememberNavigator
import org.resonate.android.ui.navigation.rootScreens
import org.resonate.android.ui.player.LocalPlayerExpansion
import org.resonate.android.ui.player.MiniPlayer
import org.resonate.android.ui.player.NowPlaying
import org.resonate.android.ui.player.PlayerViewModel
import org.resonate.android.ui.player.rememberPlayerExpansion
import org.resonate.android.ui.theme.ResonateTheme

private class Destination(val label: String, val selected: ImageVector, val unselected: ImageVector)

private val destinations =
    mapOf(
        Screen.Home to Destination("Home", Icons.Rounded.Home, Icons.Outlined.Home),
        Screen.Library to
            Destination("Library", Icons.Rounded.LibraryMusic, Icons.Outlined.LibraryMusic),
        Screen.Search to Destination("Search", Icons.Rounded.Search, Icons.Outlined.Search),
    )

@Composable
fun ResonateApp(resonate: Resonate) {
    val appearance by resonate.features.appearance.appearance.collectAsStateWithLifecycle()
    ResonateTheme(appearance) {
        val navigator = rememberNavigator()
        val snackbar = remember { SnackbarHostState() }
        val sheets = remember { Sheets(snackbar) }
        val clock = remember { ArtworkClock() }
        val expansion = rememberPlayerExpansion()
        navigator.beforeOpen = expansion::collapse
        CompositionLocalProvider(
            LocalResonate provides resonate,
            LocalNavigator provides navigator,
            LocalSheets provides sheets,
            LocalArtworkClock provides clock,
            LocalPlayerExpansion provides expansion,
        ) {
            FollowArtworkChanges(clock)
            val player = resonateViewModel { PlayerViewModel(it) }
            val adaptive = currentWindowAdaptiveInfo()
            val fitted = NavigationSuiteScaffoldDefaults.navigationSuiteType(adaptive)
            val short = !adaptive.windowSizeClass.isHeightAtLeastBreakpoint(SHORT_WINDOW_BELOW_DP)
            val suite =
                if (short && fitted != NavigationSuiteType.WideNavigationRailCollapsed)
                    NavigationSuiteType.NavigationRail
                else fitted
            Box(
                Modifier.fillMaxSize()
                    .background(MaterialTheme.colorScheme.surface)
                    .semantics { testTagsAsResourceId = true }
            ) {
                NavigationSuiteScaffold(
                    navigationSuiteType = suite,
                    containerColor = MaterialTheme.colorScheme.surface,
                    navigationItems = {
                        rootScreens.forEach { screen ->
                            val destination = destinations.getValue(screen)
                            val selected = navigator.root == screen
                            NavigationSuiteItem(
                                selected = selected,
                                onClick = { navigator.select(screen) },
                                icon = {
                                    Icon(
                                        if (selected) destination.selected
                                        else destination.unselected,
                                        contentDescription = null,
                                    )
                                },
                                label = { Text(destination.label) },
                                navigationSuiteType = suite,
                                modifier = Modifier.testTag("tab-${destination.label.lowercase()}"),
                            )
                        }
                    },
                ) {
                    val bottom =
                        if (
                            suite == NavigationSuiteType.WideNavigationRailCollapsed ||
                                suite == NavigationSuiteType.NavigationRail
                        )
                            Modifier.windowInsetsPadding(
                                WindowInsets.navigationBars.only(WindowInsetsSides.Bottom)
                            )
                        else Modifier
                    Column(Modifier.fillMaxSize().then(bottom)) {
                        Box(Modifier.weight(1f)) {
                            NavDisplay(
                                entries = navigator.entries { ScreenContent(it) },
                                onBack = navigator::back,
                                transitionSpec = {
                                    (slideInHorizontally(tween(300)) { it / 6 } + fadeIn(tween(300))) togetherWith
                                        fadeOut(tween(150))
                                },
                                popTransitionSpec = {
                                    fadeIn(tween(300)) togetherWith
                                        (slideOutHorizontally(tween(300)) { it / 6 } + fadeOut(tween(200)))
                                },
                            )
                            SnackbarHost(
                                snackbar,
                                modifier = Modifier.align(Alignment.BottomCenter).padding(bottom = 8.dp),
                            )
                        }
                        Banners()
                        MiniPlayer(player, expansion)
                    }
                }
                if (expansion.shown) NowPlaying(player, expansion)
                SheetHost()
            }
        }
    }
}

private const val SHORT_WINDOW_BELOW_DP = 480
