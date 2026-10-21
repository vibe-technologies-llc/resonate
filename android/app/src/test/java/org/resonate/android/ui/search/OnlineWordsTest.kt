package org.resonate.android.ui.search

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import org.resonate.android.nativecore.SearchSong
import org.resonate.android.nativecore.WantedSong

class OnlineWordsTest {
    private fun song(title: String, held: Boolean = false, durationMs: ULong? = 225_000u) =
        SearchSong(
            recording = "rec-$title",
            title = title,
            artist = "Ivy Cole",
            album = "Copper Sun",
            durationMs = durationMs,
            isrcs = emptyList(),
            held = held,
        )

    @Test
    fun aSongLineNamesArtistAlbumAndLength() {
        assertEquals("Ivy Cole · Copper Sun · 3:45", songLine(song("a")))
        assertEquals("Ivy Cole · Copper Sun", songLine(song("a", durationMs = null)))
    }

    @Test
    fun theSummaryCountsSavedAndHeldSongs() {
        assertNull(wantedSummary(emptyList()))
        assertEquals("1 song", wantedSummary(listOf(WantedSong(1u, song("a"), false))))
        assertEquals(
            "2 songs · 1 in your library",
            wantedSummary(listOf(WantedSong(1u, song("a"), true), WantedSong(2u, song("b"), false))),
        )
    }

    @Test
    fun megabytesHaveOneDecimal() {
        assertEquals("1.5 MB", megabytes(1_572_864u))
        assertEquals("0.0 MB", megabytes(0u))
    }
}
