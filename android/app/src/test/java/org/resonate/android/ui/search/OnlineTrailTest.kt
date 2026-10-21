package org.resonate.android.ui.search

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class OnlineTrailTest {
    @Test
    fun aTrailStartsAtTheTopResults() {
        val trail = OnlineTrail()

        assertEquals(OnlineView.Top, trail.top)
        assertFalse(trail.nested)
        assertEquals(trail, trail.back())
    }

    @Test
    fun drillingDownAndBackRetracesTheSteps() {
        val all = OnlineView.Found(OnlineKind.ALBUMS)
        val editions = OnlineView.Editions("group", "Lumen", "Aurora Fields")
        val release = OnlineView.Release("release", "Lumen", "2021 · GB")

        val trail = OnlineTrail().opened(all).opened(editions).opened(release)

        assertEquals(release, trail.top)
        assertEquals(editions, trail.back().top)
        assertEquals(all, trail.back().back().top)
        assertEquals(OnlineView.Top, trail.back().back().back().top)
        assertTrue(trail.nested)
    }

    @Test
    fun openingTheShownViewAgainAddsNoStep() {
        val artist = OnlineView.Discography("id", "Ivy Cole")

        val trail = OnlineTrail().opened(artist).opened(artist)

        assertEquals(2, trail.views.size)
    }

    @Test
    fun aNewQueryRootsTheTrailAtTheTop() {
        val trail = OnlineTrail().opened(OnlineView.Discography("id", "Ivy Cole"))

        val rooted = trail.rooted("night")

        assertEquals(listOf<OnlineView>(OnlineView.Top), rooted.views)
    }

    @Test
    fun aLinkRootsALinkViewAndTextRootsTheTopAgain() {
        val trail = OnlineTrail().rooted(" https://tidal.com/album/1 ")

        assertEquals(OnlineView.Link("https://tidal.com/album/1"), trail.top)
        assertEquals(OnlineView.Top, trail.rooted("night").top)
    }

    @Test
    fun eachViewAsksForWhatItShows() {
        assertEquals(OnlineRequest.Songs("night"), shows(OnlineView.Found(OnlineKind.SONGS), " night "))
        assertEquals(OnlineRequest.Albums("night"), shows(OnlineView.Found(OnlineKind.ALBUMS), "night"))
        assertEquals(OnlineRequest.Artists("night"), shows(OnlineView.Found(OnlineKind.ARTISTS), "night"))
        assertEquals(OnlineRequest.Discography("id"), shows(OnlineView.Discography("id", "Ivy"), ""))
        assertEquals(OnlineRequest.Editions("g"), shows(OnlineView.Editions("g", "t", "a"), ""))
        assertEquals(OnlineRequest.Release("r"), shows(OnlineView.Release("r", "t", ""), ""))
        assertEquals(OnlineRequest.Link("https://x"), shows(OnlineView.Link("https://x"), "other"))
        assertNull(shows(OnlineView.Top, "night"))
    }

    @Test
    fun resultsWaitForTwoLettersAndNeverSearchALink() {
        assertNull(shows(OnlineView.Found(OnlineKind.SONGS), "a"))
        assertNull(shows(OnlineView.Found(OnlineKind.SONGS), "https://tidal.com/track/1"))
    }
}
