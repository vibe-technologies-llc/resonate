package org.resonate.android

import org.junit.Assert.assertEquals
import org.junit.Test
import org.resonate.android.nativecore.LossySong
import org.resonate.android.nativecore.Track

class LossySongsTest {
    private fun track(id: ULong, codec: String) =
        Track(
            id = id,
            uri = "content://shelf/$id",
            name = "",
            title = "",
            artist = "",
            album = "",
            durationMs = 0uL,
            rate = 0u,
            codec = codec,
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

    private fun lossy(id: ULong, lossless: Track? = null, wantable: Boolean = false, pending: Boolean = false) =
        LossySong(track(id, "MP3"), lossless, wantable, pending)

    @Test
    fun aSongIsTickedWhereItsCopyIsHeldOrCanBeWanted() {
        val songs =
            listOf(
                lossy(1uL, lossless = track(9uL, "FLAC"), wantable = true, pending = true),
                lossy(2uL, wantable = true),
                lossy(3uL, wantable = true, pending = true),
                lossy(4uL),
            )

        assertEquals(
            listOf(LossyStanding.HELD, LossyStanding.WANTABLE, LossyStanding.WAITING, LossyStanding.UNMATCHED),
            songs.map(LossySongs::standing),
        )
        assertEquals(setOf(1uL, 2uL), LossySongs.ticked(songs))
        assertEquals(setOf(2uL), LossySongs.stillTicked(songs, setOf(2uL, 3uL, 4uL)))
        assertEquals(1, LossySongs.deletes(songs, setOf(1uL, 2uL)))
    }

    @Test
    fun eachRowSaysWhatReplacingItWouldDo() {
        assertEquals(
            "MP3; a lossless FLAC copy is held",
            LossySongs.line(English, lossy(1uL, lossless = track(9uL, "FLAC"))),
        )
        assertEquals("MP3; a lossless copy is downloaded first", LossySongs.line(English, lossy(2uL, wantable = true)))
        assertEquals(
            "MP3; waiting for a lossless copy to download",
            LossySongs.line(English, lossy(3uL, wantable = true, pending = true)),
        )
        assertEquals(
            "MP3; not matched on MusicBrainz, so no lossless copy can be asked for",
            LossySongs.line(English, lossy(4uL)),
        )
    }

    @Test
    fun whatWasDoneIsCountedPartByPart() {
        assertEquals(
            "Replaced 2 songs with their lossless copies; 1 lossless copy wanted; " +
                "1 file could not be deleted; its folder would not let go of it",
            LossySongs.told(English, LossyReplaced(replaced = 2, wanted = 1, unmatched = 0, kept = 1)),
        )
        assertEquals(
            "3 songs not matched, so nothing could be asked for",
            LossySongs.told(English, LossyReplaced(replaced = 0, wanted = 0, unmatched = 3, kept = 0)),
        )
        assertEquals("Nothing was replaced", LossySongs.told(English, LossyReplaced(0, 0, 0, 0)))
    }
}
