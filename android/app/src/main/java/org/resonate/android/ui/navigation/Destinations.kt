package org.resonate.android.ui.navigation

import androidx.compose.runtime.Composable
import org.resonate.android.ui.equaliser.EqualiserScreen
import org.resonate.android.ui.equaliser.HeadphoneCorrectionScreen
import org.resonate.android.ui.home.HomeScreen
import org.resonate.android.ui.library.AlbumScreen
import org.resonate.android.ui.library.ArtistScreen
import org.resonate.android.ui.library.FavouritesScreen
import org.resonate.android.ui.library.LibraryScreen
import org.resonate.android.ui.library.MixScreen
import org.resonate.android.ui.library.PlaylistScreen
import org.resonate.android.ui.search.SearchScreen
import org.resonate.android.ui.search.WantedScreen
import org.resonate.android.ui.settings.AppearanceScreen
import org.resonate.android.ui.settings.FoldersScreen
import org.resonate.android.ui.settings.LibraryToolsScreen
import org.resonate.android.ui.settings.OutputScreen
import org.resonate.android.ui.settings.ProcessingScreen
import org.resonate.android.ui.settings.ProviderAccountScreen
import org.resonate.android.ui.settings.ProvidersScreen
import org.resonate.android.ui.settings.ScrobblingScreen
import org.resonate.android.ui.settings.SettingsScreen
import org.resonate.android.ui.settings.StatisticsScreen
import org.resonate.android.ui.settings.IdenticalSongsScreen
import org.resonate.android.ui.settings.SuggestionsScreen
import org.resonate.android.ui.settings.SyncScreen
import org.resonate.android.ui.tools.AnalysisScreen
import org.resonate.android.ui.tools.DetailsScreen
import org.resonate.android.ui.tools.LyricsScreen
import org.resonate.android.ui.tools.VisualizerScreen

@Composable
fun ScreenContent(screen: Screen) {
    when (screen) {
        Screen.Home -> HomeScreen()
        Screen.Library -> LibraryScreen()
        Screen.Search -> SearchScreen()
        Screen.Settings -> SettingsScreen()
        is Screen.Album -> AlbumScreen(screen)
        is Screen.Artist -> ArtistScreen(screen)
        is Screen.Playlist -> PlaylistScreen(screen)
        Screen.Favourites -> FavouritesScreen()
        is Screen.Mix -> MixScreen(screen.mix)
        is Screen.Lyrics -> LyricsScreen(screen.track)
        is Screen.Analysis -> AnalysisScreen(screen.track)
        is Screen.Details -> DetailsScreen(screen.track)
        Screen.Visualizer -> VisualizerScreen()
        Screen.Equaliser -> EqualiserScreen()
        Screen.HeadphoneCorrection -> HeadphoneCorrectionScreen()
        Screen.Processing -> ProcessingScreen()
        Screen.Output -> OutputScreen()
        Screen.Folders -> FoldersScreen()
        Screen.LibraryTools -> LibraryToolsScreen()
        Screen.Statistics -> StatisticsScreen()
        Screen.Suggestions -> SuggestionsScreen()
        Screen.IdenticalSongs -> IdenticalSongsScreen()
        Screen.Wanted -> WantedScreen()
        Screen.Providers -> ProvidersScreen()
        is Screen.ProviderAccount -> ProviderAccountScreen(screen)
        Screen.Scrobbling -> ScrobblingScreen()
        Screen.Sync -> SyncScreen()
        Screen.Appearance -> AppearanceScreen()
    }
}
