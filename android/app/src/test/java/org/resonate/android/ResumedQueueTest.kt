package org.resonate.android

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
}
