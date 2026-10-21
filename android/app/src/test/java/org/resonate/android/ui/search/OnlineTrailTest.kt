package org.resonate.android.ui.search

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class OnlineTrailTest {
    @Test
    fun aTrailStartsAtSongResults() {
        val trail = OnlineTrail()

        assertEquals(OnlineView.Found(OnlineKind.SONGS), trail.top)
        assertFalse(trail.nested)
        assertEquals(trail, trail.back())
    }

    @Test
    fun drillingDownAndBackRetracesTheSteps() {
        val editions = OnlineView.Editions("group", "Lumen", "Aurora Fields")
        val release = OnlineView.Release("release", "Lumen", "2021 · GB")

        val trail = OnlineTrail(OnlineKind.ALBUMS).opened(editions).opened(release)

        assertEquals(release, trail.top)
        assertEquals(editions, trail.back().top)
        assertEquals(OnlineView.Found(OnlineKind.ALBUMS), trail.back().back().top)
        assertTrue(trail.nested)
    }

    @Test
    fun openingTheShownViewAgainAddsNoStep() {
        val artist = OnlineView.Discography("id", "Ivy Cole")

        val trail = OnlineTrail().opened(artist).opened(artist)

        assertEquals(2, trail.views.size)
    }

    @Test
    fun aNewQueryRootsTheTrailAndKeepsTheKind() {
        val trail = OnlineTrail(OnlineKind.ARTISTS).opened(OnlineView.Discography("id", "Ivy Cole"))

        val rooted = trail.rooted("night")

        assertEquals(listOf<OnlineView>(OnlineView.Found(OnlineKind.ARTISTS)), rooted.views)
        assertEquals(OnlineKind.ARTISTS, rooted.kind)
    }

    @Test
    fun aLinkRootsALinkViewAndTheKindSurvivesIt() {
        val trail = OnlineTrail(OnlineKind.ALBUMS).rooted(" https://tidal.com/album/1 ")

        assertEquals(OnlineView.Link("https://tidal.com/album/1"), trail.top)
        assertEquals(OnlineView.Found(OnlineKind.ALBUMS), trail.rooted("night").top)
    }

    @Test
    fun choosingAKindRootsThatKind() {
        val trail = OnlineTrail().rooted("night", OnlineKind.ALBUMS)

        assertEquals(OnlineView.Found(OnlineKind.ALBUMS), trail.top)
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
    }

    @Test
    fun resultsWaitForTwoLettersAndNeverSearchALink() {
        assertNull(shows(OnlineView.Found(OnlineKind.SONGS), "a"))
        assertNull(shows(OnlineView.Found(OnlineKind.SONGS), "https://tidal.com/track/1"))
    }
}
