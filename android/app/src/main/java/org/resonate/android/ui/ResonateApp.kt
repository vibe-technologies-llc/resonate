package org.resonate.android.ui

import androidx.annotation.StringRes
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
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Home
import androidx.compose.material.icons.outlined.LibraryMusic
import androidx.compose.material.icons.outlined.Search
import androidx.compose.material.icons.rounded.GroupAdd
import androidx.compose.material.icons.rounded.Headphones
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
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.hideFromAccessibility
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.testTagsAsResourceId
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.navigation3.ui.NavDisplay
import org.resonate.android.ArrivalPage
import org.resonate.android.R
import org.resonate.android.ui.components.ArtworkClock
import org.resonate.android.ui.components.Banners
import org.resonate.android.ui.components.FollowArtworkChanges
import org.resonate.android.ui.components.LocalArtworkClock
import org.resonate.android.ui.components.LocalPageBottomPadding
import org.resonate.android.ui.components.LocalSheets
import org.resonate.android.ui.components.SheetHost
import org.resonate.android.ui.components.Sheets
import org.resonate.android.ui.components.StatusNotice
import org.resonate.android.ui.navigation.LocalNavigator
import org.resonate.android.ui.navigation.Screen
import org.resonate.android.ui.navigation.ScreenContent
import org.resonate.android.ui.navigation.isServed
import org.resonate.android.ui.navigation.rememberNavigator
import org.resonate.android.ui.navigation.rootScreens
import org.resonate.android.ui.player.LocalPlayerExpansion
import org.resonate.android.ui.player.MiniPlayer
import org.resonate.android.ui.player.NowPlaying
import org.resonate.android.ui.player.PlayerViewModel
import org.resonate.android.ui.player.rememberPlayerExpansion
import org.resonate.android.ui.settings.ConfirmDialog
import org.resonate.android.ui.theme.ResonateTheme

private class Destination(
    @StringRes val label: Int,
    val tag: String,
    val selected: ImageVector,
    val unselected: ImageVector,
)

private val destinations =
    mapOf(
        Screen.Home to
            Destination(R.string.shell_tab_home, "home", Icons.Rounded.Home, Icons.Outlined.Home),
        Screen.Library to
            Destination(
                R.string.shell_tab_library,
                "library",
                Icons.Rounded.LibraryMusic,
                Icons.Outlined.LibraryMusic,
            ),
        Screen.Search to
            Destination(
                R.string.shell_tab_search,
                "search",
                Icons.Rounded.Search,
                Icons.Outlined.Search,
            ),
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
            val serving by resonate.features.served.serving.collectAsStateWithLifecycle()
            LaunchedEffect(serving) {
                if (!serving) navigator.leave { it.isServed }
            }
            val player = resonateViewModel { PlayerViewModel(it) }
            val adaptive = currentWindowAdaptiveInfo()
            val fitted = NavigationSuiteScaffoldDefaults.navigationSuiteType(adaptive)
            val short = !adaptive.windowSizeClass.isHeightAtLeastBreakpoint(SHORT_WINDOW_BELOW_DP)
            val suite =
                if (short && fitted != NavigationSuiteType.WideNavigationRailCollapsed)
                    NavigationSuiteType.NavigationRail
                else fitted
            Box(
                Modifier.fillMaxSize().background(MaterialTheme.colorScheme.surface).semantics {
                    testTagsAsResourceId = true
                }
            ) {
                NavigationSuiteScaffold(
                    modifier =
                        if (expansion.shown)
                            Modifier.clearAndSetSemantics { hideFromAccessibility() }
                        else Modifier,
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
                                label = { Text(stringResource(destination.label)) },
                                navigationSuiteType = suite,
                                modifier = Modifier.testTag("tab-${destination.tag}"),
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
                    var footerHeight by remember { mutableIntStateOf(0) }
                    val footerPadding = with(LocalDensity.current) { footerHeight.toDp() }
                    Box(Modifier.fillMaxSize().then(bottom)) {
                        CompositionLocalProvider(LocalPageBottomPadding provides footerPadding) {
                            NavDisplay(
                                entries = navigator.entries { ScreenContent(it) },
                                onBack = navigator::back,
                                transitionSpec = {
                                    (slideInHorizontally(tween(300)) { it / 6 } +
                                        fadeIn(tween(300))) togetherWith fadeOut(tween(150))
                                },
                                popTransitionSpec = {
                                    fadeIn(tween(300)) togetherWith
                                        (slideOutHorizontally(tween(300)) { it / 6 } +
                                            fadeOut(tween(200)))
                                },
                                predictivePopTransitionSpec = {
                                    slideInHorizontally(tween(300)) { -it / 6 } togetherWith
                                        slideOutHorizontally(tween(300)) { it }
                                },
                            )
                        }
                        Column(
                            Modifier.align(Alignment.BottomCenter),
                            horizontalAlignment = Alignment.CenterHorizontally,
                        ) {
                            if (!expansion.shown) {
                                SnackbarHost(snackbar, modifier = Modifier.padding(bottom = 8.dp))
                            }
                            Column(Modifier.onSizeChanged { footerHeight = it.height }) {
                                Banners(showNotice = !expansion.shown)
                                MiniPlayer(player, expansion)
                            }
                        }
                    }
                }
                if (expansion.shown) {
                    NowPlaying(player, expansion)
                    Column(
                        modifier =
                            Modifier.align(Alignment.BottomCenter)
                                .safeDrawingPadding()
                                .padding(bottom = 8.dp)
                    ) {
                        SnackbarHost(snackbar)
                        StatusNotice()
                    }
                }
                SheetHost()
                PairingOffer()
                PartyOffer()
                InvitationArrivals()
            }
        }
    }
}

@Composable
private fun PairingOffer() {
    val resonate = LocalResonate.current
    val navigator = LocalNavigator.current
    val features = resonate.features
    val code by features.pairingOffered.collectAsStateWithLifecycle()
    val config by features.syncConfig.collectAsStateWithLifecycle()
    val offered = code ?: return
    val leaving = config?.key?.isNotEmpty() == true
    ConfirmDialog(
        title = stringResource(R.string.shell_join_devices_asks),
        text =
            if (leaving) stringResource(R.string.shell_join_devices_leaving, offered)
            else stringResource(R.string.shell_join_devices_says, offered),
        confirm = stringResource(R.string.shell_join),
        icon = Icons.Rounded.GroupAdd,
        onConfirm = {
            features.joinSync(offered)
            navigator.open(Screen.Sync)
        },
        onDismiss = { features.pairingOffered.value = null },
    )
}

@Composable
private fun PartyOffer() {
    val together = LocalResonate.current.features.together
    val link by together.partyOffered.collectAsStateWithLifecycle()
    val offered = link ?: return
    ConfirmDialog(
        title = stringResource(R.string.together_join_party_asks),
        text = stringResource(R.string.together_join_party_says),
        confirm = stringResource(R.string.shell_join),
        icon = Icons.Rounded.Headphones,
        onConfirm = { together.arrive(offered) },
        onDismiss = { together.partyOffered.value = null },
    )
}

@Composable
private fun InvitationArrivals() {
    val together = LocalResonate.current.features.together
    val navigator = LocalNavigator.current
    val arrived by together.arrived.collectAsStateWithLifecycle()
    LaunchedEffect(arrived) {
        when (arrived) {
            ArrivalPage.PARTY -> navigator.open(Screen.Party)
            ArrivalPage.PLAYLISTS -> {
                navigator.beforeOpen()
                navigator.selectAtRoot(Screen.Library)
                together.playlistsAsked.value = true
            }
            null -> return@LaunchedEffect
        }
        together.arrived.value = null
    }
}

private const val SHORT_WINDOW_BELOW_DP = 480
