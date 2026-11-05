package org.resonate.android.ui.settings

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import org.resonate.android.nativecore.RemoteAnswer
import org.resonate.android.nativecore.RemoteControl
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

    @Test
    fun theRowsShownStartAboutTheOnePlayingAndKeepTheirPlaceInTheQueue() {
        val many =
            zone(640u, 20u)
                .copy(
                    firstRow = 5u,
                    queueLength = 40u,
                    rows = (5..39).map { RemoteSong("$it", "", null) },
                )

        val shown = DrivenWords.rowsShown(many, 4)

        assertEquals(listOf(18u, 19u, 20u, 21u), shown.map { it.row })
        assertEquals("20", shown[2].song.title)
        assertEquals(listOf(false, false, true, false), shown.map { it.playing })
        assertEquals(13, DrivenWords.rowsBefore(many))
        assertEquals(listOf(2u, 3u), DrivenWords.rowsShown(zone(640u, 2u), 12).map { it.row })
    }

    @Test
    fun aRowMovesOnlyWithinItsQueueAndASeekIsHeldWithinTheSong() {
        val zone = zone(640u, 2u)

        assertNull(DrivenWords.moveUp(0u))
        assertEquals(RemoteControl.Move(4u, 3u), DrivenWords.moveUp(4u))
        assertNull(DrivenWords.moveDown(3u, 4u))
        assertEquals(RemoteControl.Move(2u, 3u), DrivenWords.moveDown(2u, 4u))
        assertEquals(RemoteControl.Seek(3_725_000uL), DrivenWords.seekTo(zone, 2f))
        assertEquals(RemoteControl.Seek(0uL), DrivenWords.seekTo(zone, -1f))
        assertNull(DrivenWords.seekTo(zone.copy(lengthMillis = null), 0.5f))
        assertEquals(0f, DrivenWords.share(zone.copy(lengthMillis = 0uL)))
    }
}
