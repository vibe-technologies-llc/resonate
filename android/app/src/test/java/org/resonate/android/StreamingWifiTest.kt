package org.resonate.android

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.resonate.android.nativecore.Track
import org.resonate.android.nativecore.Transport

class StreamingWifiTest {
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

    @Test
    fun aServedSongPlayingOrLoadingHoldsTheLockAndNothingElseDoes() {
        val served = track(0uL, "subsonic:track/so-1")
        val local = track(4uL, "content://music/a.flac")

        assertTrue(streamsOverTheNetwork(served, Transport.PLAYING))
        assertTrue(streamsOverTheNetwork(served, Transport.LOADING))
        assertFalse(streamsOverTheNetwork(served, Transport.PAUSED))
        assertFalse(streamsOverTheNetwork(served, Transport.STOPPED))
        assertFalse(streamsOverTheNetwork(local, Transport.PLAYING))
        assertFalse(streamsOverTheNetwork(null, Transport.PLAYING))
    }
}
