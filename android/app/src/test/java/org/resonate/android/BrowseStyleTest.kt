package org.resonate.android

import org.junit.Assert.assertEquals
import org.junit.Test

class BrowseStyleTest {
    @Test
    fun collectionsOfCoversAreGridsAndTheirSongsLists() {
        val grid = BrowseStyle(BrowseStyle.GRID, BrowseStyle.LIST)

        assertEquals(grid, BrowseStyle.ofChildren(BrowseNode.Albums))
        assertEquals(grid, BrowseStyle.ofChildren(BrowseNode.Artists))
        assertEquals(grid, BrowseStyle.ofChildren(BrowseNode.Playlists))
    }

    @Test
    fun theRootAndEveryFolderOfSongsAreLists() {
        val list = BrowseStyle(BrowseStyle.LIST, BrowseStyle.LIST)

        assertEquals(list, BrowseStyle.ofChildren(BrowseNode.Root))
        assertEquals(list, BrowseStyle.ofChildren(BrowseNode.Favourites))
        assertEquals(list, BrowseStyle.ofChildren(BrowseNode.Album("a")))
        assertEquals(list, BrowseStyle.ofChildren(BrowseNode.Artist("a")))
        assertEquals(list, BrowseStyle.ofChildren(BrowseNode.Playlist(1uL)))
    }
}
