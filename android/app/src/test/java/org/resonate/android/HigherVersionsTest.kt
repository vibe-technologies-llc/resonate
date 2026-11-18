package org.resonate.android

import org.junit.Assert.assertEquals
import org.junit.Test
import org.resonate.android.nativecore.FoundVersion
import org.resonate.android.nativecore.HigherVersion
import org.resonate.android.nativecore.HigherVersionPicked
import org.resonate.android.nativecore.SongVersion
import org.resonate.android.nativecore.Track

class HigherVersionsTest {
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

    private val held = FoundVersion(SongVersion.Held(9uL), "Play the FLAC 24-bit / 96 kHz copy")

    private val served = FoundVersion(SongVersion.Served("tidal:track/7"), "Download the FLAC version from TIDAL")

    private fun higher(id: ULong, found: FoundVersion? = null, chosen: Boolean = false, pending: Boolean = false) =
        HigherVersion(track(id, "MP3"), found, chosen, pending)

    @Test
    fun onlyAVersionFoundAndNotYetChosenIsTickedAndPicked() {
        val songs =
            listOf(
                higher(1uL, held),
                higher(2uL, served),
                higher(3uL, held, chosen = true),
                higher(4uL, pending = true),
                higher(5uL),
            )

        assertEquals(
            listOf(
                HigherStanding.FOUND,
                HigherStanding.FOUND,
                HigherStanding.CHOSEN,
                HigherStanding.WAITING,
                HigherStanding.NONE,
            ),
            songs.map(HigherVersions::standing),
        )
        assertEquals(setOf(1uL, 2uL), HigherVersions.ticked(songs))
        assertEquals(setOf(2uL), HigherVersions.stillTicked(songs, setOf(2uL, 3uL, 4uL)))
        assertEquals(
            listOf(HigherVersionPicked(2uL, served.version)),
            HigherVersions.picked(songs, setOf(2uL, 3uL, 5uL)),
        )
    }

    @Test
    fun eachRowSaysWhatWasFound() {
        assertEquals(
            "MP3; Play the FLAC 24-bit / 96 kHz copy",
            HigherVersions.line(English, higher(1uL, held)),
        )
        assertEquals(
            "MP3; already plays Play the FLAC 24-bit / 96 kHz copy",
            HigherVersions.line(English, higher(1uL, held, chosen = true)),
        )
        assertEquals(
            "MP3; waiting for its version to download",
            HigherVersions.line(English, higher(3uL, pending = true)),
        )
        assertEquals(
            "MP3; no higher quality version held or on your services",
            HigherVersions.line(English, higher(4uL)),
        )
    }

    @Test
    fun whatWasKeptIsCountedPartByPart() {
        assertEquals(
            "2 songs play their higher quality versions from now on; 1 version is downloaded first",
            HigherVersions.told(English, HigherKept(chosen = 2, downloading = 1, passedOver = 0)),
        )
        assertEquals(
            "3 versions could no longer be kept",
            HigherVersions.told(English, HigherKept(chosen = 0, downloading = 0, passedOver = 3)),
        )
        assertEquals("Nothing was kept", HigherVersions.told(English, HigherKept(0, 0, 0)))
    }
}
