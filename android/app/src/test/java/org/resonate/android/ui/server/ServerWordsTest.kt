package org.resonate.android.ui.server

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
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
    ) = ServedSong("subsonic:track/1", title, artist, album, track, milliseconds, null, null)

    @Test
    fun anArtistCountsItsAlbumsWhenTheServerSaysHowMany() {
        assertEquals("1 album", artistLine(ServedArtist("subsonic:artist/1", "Air", 1u, null)))
        assertEquals("3 albums", artistLine(ServedArtist("subsonic:artist/1", "Air", 3u, null)))
        assertEquals(null, artistLine(ServedArtist("subsonic:artist/1", "Air", null, null)))
    }

    @Test
    fun anAlbumNamesItsArtistYearAndSongsAndSkipsWhatTheServerLeftOut() {
        val full = ServedAlbum("subsonic:album/1", "Moon Safari", "Air", 1998.toUShort(), 10u, null)
        val bare = ServedAlbum("subsonic:album/2", "", " ", null, null, null)

        assertEquals("Air · 1998 · 10 songs", albumLine(full))
        assertEquals("", albumLine(bare))
        assertEquals("Unknown album", bare.shownName)
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
        assertEquals("2 songs · 7 min", albumSummary(listOf(song(), song())))
        assertEquals("1 song", albumSummary(listOf(song(milliseconds = null))))
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
        assertEquals("1 artist · 2 songs", foundSummary(some))
    }
}
