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

    @Test
    fun aSongIdFromAnyFolderOrAPlainIdNamesItsTrack() {
        assertEquals(12uL, BrowseIds.trackOf(BrowseIds.song(12uL, BrowseIds.ROOT)))
        assertEquals(12uL, BrowseIds.trackOf(BrowseIds.song(12uL, BrowseIds.playlist(3uL))))
        assertEquals(12uL, BrowseIds.trackOf("12"))
        assertNull(BrowseIds.trackOf(BrowseIds.ALBUMS))
        assertNull(BrowseIds.trackOf("x@root"))
    }

    @Test
    fun anUnreadableIdIsLeftOutAndTheStartFollowsTheSongItNamed() {
        val ids = listOf("albums", "4@root", "nothing", "7", "9@favourites")

        assertEquals(listOf(4uL, 7uL, 9uL) to 1, BrowseIds.playable(ids, 3))
        assertEquals(listOf(4uL, 7uL, 9uL) to 0, BrowseIds.playable(ids, 0))
        assertEquals(listOf(4uL, 7uL, 9uL) to 2, BrowseIds.playable(ids, 9))
        assertEquals(emptyList<ULong>() to 0, BrowseIds.playable(listOf("albums"), 0))
    }
}
