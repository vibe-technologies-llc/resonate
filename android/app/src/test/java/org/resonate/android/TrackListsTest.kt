package org.resonate.android

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.resonate.android.nativecore.Track

class TrackListsTest {
    private fun song(id: ULong, favourite: Boolean = false, artRevision: ULong = 0uL) =
        Track(
            id = id,
            uri = "content://test/$id",
            name = "$id.flac",
            title = "Song $id",
            artist = "Artist",
            album = "Album",
            durationMs = 60_000uL,
            rate = 44_100u,
            codec = "FLAC",
            favourite = favourite,
            albumArtist = "",
            genre = "",
            date = "",
            trackNumber = null,
            discNumber = null,
            bits = 16u,
            channels = 2u,
            recording = null,
            release = null,
            artistMbid = null,
            artRevision = artRevision,
            queueId = null,
        )

    @Test
    fun aFavouriteToggledChangesNothingTheChannelNeedsToCarry() {
        val before = listOf(song(1uL), song(2uL))
        val after = listOf(song(1uL), song(2uL, favourite = true))

        assertTrue(sameSongs(before, after))
        assertTrue(sameSongs(after, before))
    }

    @Test
    fun anythingElseThatChangedIsCarried() {
        val before = listOf(song(1uL), song(2uL))

        assertFalse(sameSongs(before, listOf(song(1uL), song(2uL, artRevision = 1uL))))
        assertFalse(sameSongs(before, listOf(song(1uL))))
        assertFalse(sameSongs(before, listOf(song(2uL), song(1uL))))
        assertFalse(
            sameSongs(before, listOf(song(1uL), song(2uL, favourite = true, artRevision = 3uL)))
        )
    }
}
