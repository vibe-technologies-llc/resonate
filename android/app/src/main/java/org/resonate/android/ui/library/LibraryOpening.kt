package org.resonate.android.ui.library

import androidx.annotation.StringRes
import org.resonate.android.LibraryFeatures
import org.resonate.android.R
import org.resonate.android.nativecore.CollectionOrder
import org.resonate.android.ui.navigation.Navigator
import org.resonate.android.ui.navigation.Screen

enum class LibraryShelf(@StringRes val title: Int) {
    SONGS(R.string.library_shelf_songs),
    ALBUMS(R.string.library_shelf_albums),
    ARTISTS(R.string.library_shelf_artists),
    PLAYLISTS(R.string.library_shelf_playlists),
}

data class LibraryOpening(val shelf: LibraryShelf, val order: CollectionOrder? = null) {
    val albumOrder: CollectionOrder?
        get() = order.takeIf { shelf == LibraryShelf.ALBUMS }

    val artistOrder: CollectionOrder?
        get() = order.takeIf { shelf == LibraryShelf.ARTISTS }

    companion object {
        val recentlyAdded = LibraryOpening(LibraryShelf.ALBUMS, CollectionOrder.ADDED)
        val mostPlayedArtists = LibraryOpening(LibraryShelf.ARTISTS, CollectionOrder.PLAYS)
        val playlists = LibraryOpening(LibraryShelf.PLAYLISTS)
    }
}

fun Navigator.showLibrary(features: LibraryFeatures, opening: LibraryOpening) {
    beforeOpen()
    selectAtRoot(Screen.Library)
    features.libraryOpening.value = opening
}
