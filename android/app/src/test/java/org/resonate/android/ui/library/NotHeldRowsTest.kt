package org.resonate.android.ui.library

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import org.resonate.android.DownloadState
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
    fun aSongNotHeldSaysItsLengthAndAnAlbumOnlyWhereAsked() {
        assertEquals("3:45", notHeldLine(song, null, withAlbum = false))
        assertEquals("Album · 3:45", notHeldLine(song, null, withAlbum = true))
    }

    @Test
    fun aSongWithNoLengthSaysNothingOfIt() {
        assertEquals("", notHeldLine(song.copy(durationMs = null), null, withAlbum = false))
    }

    @Test
    fun aSongBeingDownloadedSaysWhereItStandsInPlaceOfItsLength() {
        assertEquals("Downloading…", notHeldLine(song, DownloadState.DOWNLOADING, withAlbum = false))
        assertEquals("Waiting to download", notHeldLine(song, DownloadState.WAITING, withAlbum = true))
    }

    @Test
    fun aSettledJobStandsForNothing() {
        assertNull(standingWords(DownloadState.DONE))
        assertNull(standingWords(DownloadState.FAILED))
        assertNull(standingWords(DownloadState.CANCELLED))
        assertNull(standingWords(null))
    }
}
