package org.resonate.android.ui.server

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.resonate.android.English
import org.resonate.android.nativecore.ServedAlbum
import org.resonate.android.nativecore.ServedArtist
import org.resonate.android.nativecore.ServedFound
import org.resonate.android.nativecore.ServedSong

class ServerWordsTest {
    private fun song(
        title: String = "Sing",
        artist: String? = "Air",
        album: String? = "Moon Safari",
        track: UInt? = null,
        milliseconds: ULong? = 215_000uL,
    ) = ServedSong("subsonic:track/1", title, artist, album, track, milliseconds, null, null, null)

    @Test
    fun anArtistCountsItsAlbumsWhenTheServerSaysHowMany() {
        assertEquals(
            "1 album",
            artistLine(English, ServedArtist("subsonic:artist/1", "Air", 1u, null)),
        )
        assertEquals(
            "3 albums",
            artistLine(English, ServedArtist("subsonic:artist/1", "Air", 3u, null)),
        )
        assertEquals(
            null,
            artistLine(English, ServedArtist("subsonic:artist/1", "Air", null, null)),
        )
    }

    @Test
    fun anAlbumNamesItsArtistYearAndSongsAndSkipsWhatTheServerLeftOut() {
        val full = ServedAlbum("subsonic:album/1", "Moon Safari", "Air", 1998.toUShort(), 10u, null)
        val bare = ServedAlbum("subsonic:album/2", "", " ", null, null, null)

        assertEquals("Air · 1998 · 10 songs", albumLine(English, full))
        assertEquals("", albumLine(English, bare))
        assertEquals("Unknown album", bare.shownName(English))
    }

    @Test
    fun aSongInAnAlbumDropsTheAlbumAndOneFoundElsewhereKeepsIt() {
        assertEquals("Air · 3:35", songLine(song(), withAlbum = false))
        assertEquals("Air · Moon Safari · 3:35", songLine(song(), withAlbum = true))
        assertEquals("", songLine(song(artist = null, album = null, milliseconds = null), withAlbum = true))
    }

    @Test
    fun aSongIsNumberedByItsTrackElseByItsPlace() {
        assertEquals("7", songNumber(song(track = 7u), 0))
        assertEquals("3", songNumber(song(), 2))
    }

    @Test
    fun anAlbumSummaryCountsSongsAndAddsTheLengthOnlyWhenKnown() {
        assertEquals("2 songs · 7 min", albumSummary(English, listOf(song(), song())))
        assertEquals("1 song", albumSummary(English, listOf(song(milliseconds = null))))
    }

    @Test
    fun aSearchThatFoundNothingIsEmptyAndOneThatFoundSomethingSaysWhat() {
        val nothing = ServedFound(emptyList(), emptyList(), emptyList())
        val some =
            ServedFound(
                listOf(ServedArtist("subsonic:artist/1", "Air", null, null)),
                emptyList(),
                listOf(song(), song()),
            )

        assertTrue(nothing.isEmpty())
        assertEquals("1 artist · 2 songs", foundSummary(English, some))
    }

    @Test
    fun theSearchAndItsEmptyPagesNameTheServiceChosen() {
        assertEquals("Search TIDAL", searchHeadline(English, "TIDAL"))
        assertEquals("Search", searchHeadline(English, ""))
        assertEquals(
            "Monochrome shows nothing until you search it. Type words above and press search.",
            firstPageEmpty(English, "Monochrome"),
        )
        assertEquals(
            "TIDAL found no artist, album or song for those words.",
            nothingMatches(English, "TIDAL"),
        )
        assertEquals(
            "This service found no artist, album or song for those words.",
            nothingMatches(English, " "),
        )
    }
}
