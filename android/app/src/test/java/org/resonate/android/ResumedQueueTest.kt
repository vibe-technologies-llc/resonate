package org.resonate.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.resonate.android.nativecore.Track

class ResumedQueueTest {
    private fun track(id: ULong, uri: String) =
        Track(
            id = id,
            uri = uri,
            name = "",
            title = "",
            artist = "",
            album = "",
            durationMs = 0uL,
            rate = 0u,
            codec = "",
            favourite = false,
            albumArtist = "",
            genre = "",
            date = "",
            trackNumber = null,
            discNumber = null,
            bits = 0u,
            channels = 0u,
            recording = null,
            release = null,
            artistMbid = null,
            artRevision = 0uL,
            queueId = null,
        )

    private val mixed =
        listOf(
            track(7uL, "content://music/a.flac"),
            track(0uL, "subsonic:track/so-1"),
            track(9uL, "content://music/b.flac"),
        )

    @Test
    fun aQueueOfLibraryAndServedSongsIsNamedByItsOwnMediaItems() {
        assertTrue(namesTheQueueHeld(mixed.map { it.mediaId() }, mixed))
    }

    @Test
    fun anotherListOrAnEmptyQueueIsNotTheQueueHeld() {
        assertFalse(namesTheQueueHeld(listOf("7", "subsonic:track/so-1"), mixed))
        assertFalse(namesTheQueueHeld(listOf("9", "subsonic:track/so-1", "7"), mixed))
        assertFalse(namesTheQueueHeld(emptyList(), emptyList()))
    }

    @Test
    fun aWindowStartingAfterRowZeroIsRestoredOntoTheSongThatWasPlaying() {
        val window = QueueRead(first = 40, total = 90, tracks = mixed)
        val playingRow = 41

        val ids = window.tracks.map { it.mediaId() }
        val windowIndex = QueueWindow.windowIndex(playingRow, window)
        val restoredRow = QueueWindow.row(windowIndex, window)

        assertEquals(listOf("7", "subsonic:track/so-1", "9"), ids)
        assertTrue(namesTheQueueHeld(ids, window.tracks))
        assertEquals(1, windowIndex)
        assertEquals("subsonic:track/so-1", ids[windowIndex])
        assertEquals(playingRow, restoredRow)
    }

    @Test
    fun aRowOutsideTheWindowIsHeldToItsEdges() {
        val window = QueueRead(first = 40, total = 90, tracks = mixed)

        assertEquals(0, QueueWindow.windowIndex(12, window))
        assertEquals(2, QueueWindow.windowIndex(80, window))
        assertEquals(40, QueueWindow.row(-3, window))
        assertEquals(42, QueueWindow.row(9, window))
    }
}
