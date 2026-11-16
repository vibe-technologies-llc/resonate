package org.resonate.android.ui.library

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import org.resonate.android.DownloadState
import org.resonate.android.English
import org.resonate.android.nativecore.SearchSong

class NotHeldRowsTest {
    private val song =
        SearchSong(
            recording = "rec",
            title = "Title",
            artist = "Artist",
            album = "Album",
            durationMs = 225_000uL,
            isrcs = emptyList(),
        )

    @Test
    fun aSongNotHeldSaysItsArtistAndLengthAndAnAlbumOnlyWhereAsked() {
        assertEquals("Artist · 3:45", notHeldLine(English, song, null, withAlbum = false))
        assertEquals("Artist · Album · 3:45", notHeldLine(English, song, null, withAlbum = true))
    }

    @Test
    fun aSongWithNoLengthOrArtistSaysNothingOfIt() {
        assertEquals("Artist", notHeldLine(English, song.copy(durationMs = null), null, withAlbum = false))
        assertEquals(
            "",
            notHeldLine(English, song.copy(artist = "", durationMs = null), null, withAlbum = false),
        )
    }

    @Test
    fun aSongBeingDownloadedSaysWhereItStandsInPlaceOfItsLength() {
        assertEquals(
            "Downloading…",
            notHeldLine(English, song, DownloadState.DOWNLOADING, withAlbum = false),
        )
        assertEquals(
            "Waiting to download",
            notHeldLine(English, song, DownloadState.WAITING, withAlbum = true),
        )
    }

    @Test
    fun aSettledJobStandsForNothing() {
        assertNull(standingWords(English, DownloadState.DONE))
        assertNull(standingWords(English, DownloadState.FAILED))
        assertNull(standingWords(English, DownloadState.CANCELLED))
        assertNull(standingWords(English, null))
    }
}
