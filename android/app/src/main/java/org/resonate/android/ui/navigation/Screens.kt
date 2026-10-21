package org.resonate.android.ui.navigation

import androidx.navigation3.runtime.NavKey
import kotlinx.serialization.Serializable

@Serializable
sealed interface Screen : NavKey {
    @Serializable data object Home : Screen

    @Serializable data object Library : Screen

    @Serializable data object Search : Screen

    @Serializable data object Settings : Screen

    @Serializable
    data class Album(
        val key: String,
        val title: String,
        val artist: String,
        val artworkTrack: ULong? = null,
    ) : Screen

    @Serializable data class Artist(val name: String, val artworkTrack: ULong? = null) : Screen

    @Serializable data class Playlist(val id: ULong, val name: String) : Screen

    @Serializable data object Favourites : Screen

    @Serializable data class Mix(val mix: org.resonate.android.ui.Mix) : Screen

    @Serializable data class Lyrics(val track: ULong) : Screen

    @Serializable data class Analysis(val track: ULong) : Screen

    @Serializable data class Details(val track: ULong) : Screen

    @Serializable data object Visualizer : Screen

    @Serializable data object Equaliser : Screen

    @Serializable data object HeadphoneCorrection : Screen

    @Serializable data object Processing : Screen

    @Serializable data object Output : Screen

    @Serializable data object Folders : Screen

    @Serializable data object LibraryTools : Screen

    @Serializable data object Statistics : Screen

    @Serializable data object Suggestions : Screen

    @Serializable data object Wanted : Screen

    @Serializable data object Providers : Screen

    @Serializable data object Scrobbling : Screen

    @Serializable data object Sync : Screen

    @Serializable data object Appearance : Screen
}

val rootScreens = listOf(Screen.Home, Screen.Library, Screen.Search)
