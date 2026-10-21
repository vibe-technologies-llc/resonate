package org.resonate.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class BrowseIdsTest {
    @Test
    fun aFolderIdReadsBackAsItsNode() {
        assertEquals(BrowseNode.Albums, BrowseIds.read(BrowseIds.ALBUMS))
        assertEquals(BrowseNode.Favourites, BrowseIds.read(BrowseIds.FAVOURITES))
        assertEquals(BrowseNode.Playlist(7uL), BrowseIds.read(BrowseIds.playlist(7uL)))
        assertEquals(BrowseNode.Artist("Air"), BrowseIds.read(BrowseIds.artist("Air")))
        assertNull(BrowseIds.read("playlist:seven"))
        assertNull(BrowseIds.read("nothing"))
    }

    @Test
    fun anAlbumKeyKeepsItsSeparatorAndAtSigns() {
        val key = "moon safari\u001fair@home"

        assertEquals(BrowseNode.Album(key), BrowseIds.read(BrowseIds.album(key)))
        assertEquals(12uL to BrowseIds.album(key), BrowseIds.songOf(BrowseIds.song(12uL, BrowseIds.album(key))))
    }

    @Test
    fun aPlainCatalogIdIsNotABrowsedSong() {
        assertNull(BrowseIds.songOf("12"))
        assertNull(BrowseIds.songOf("x@albums"))
    }
}
