package org.resonate.android.ui.settings

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import org.resonate.android.nativecore.RemoteAnswer
import org.resonate.android.nativecore.RemotePlaying
import org.resonate.android.nativecore.RemoteSong
import org.resonate.android.nativecore.RemoteZoneState

class DrivenWordsTest {
    private fun zone(volume: UShort, playingRow: UInt?) =
        RemoteZoneState(
            zone = 1u,
            name = "Living room",
            playing = RemotePlaying.PLAYING,
            playingRow = playingRow,
            positionMillis = 62_000uL,
            lengthMillis = 3_725_000uL,
            volumePermille = volume,
            queueLength = 4u,
            firstRow = 2u,
            rows = listOf(RemoteSong("Echoes", "Pink Floyd", null), RemoteSong("Time", "", null)),
        )

    @Test
    fun aZoneSaysItsSongPlaceAndVolumeAndStepsTheVolumeWithinItsBounds() {
        assertEquals("Time", DrivenWords.song(zone(640u, 3u)))
        assertEquals("Echoes — Pink Floyd", DrivenWords.song(zone(640u, 2u)))
        assertEquals("Nothing playing", DrivenWords.song(zone(640u, null)))
        assertEquals("1:02 / 1:02:05", DrivenWords.place(zone(640u, 2u)))
        assertEquals("64%", DrivenWords.volume(zone(640u, 2u)))
        assertEquals(1000.toUShort(), DrivenWords.louder(zone(950u, 2u)))
        assertEquals(0.toUShort(), DrivenWords.quieter(zone(40u, 2u)))
        assertNull(DrivenWords.refusal(RemoteAnswer.DONE))
        assertEquals("The other device did not answer", DrivenWords.refusal(RemoteAnswer.UNANSWERED))
    }
}
