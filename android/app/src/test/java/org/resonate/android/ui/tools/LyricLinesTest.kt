package org.resonate.android.ui.tools

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.resonate.android.nativecore.LyricDetail
import org.resonate.android.nativecore.LyricFrame
import org.resonate.android.nativecore.LyricLetter
import org.resonate.android.nativecore.LyricReads
import org.resonate.android.nativecore.LyricRow
import org.resonate.android.nativecore.LyricSheet
import org.resonate.android.nativecore.LyricWait
import org.resonate.android.nativecore.LyricWipe

class LyricLinesTest {
    private fun row(text: String, at: ULong?, voice: UInt = 1u, breathes: Boolean = false) =
        LyricRow(
            text = text,
            atMs = at,
            untilMs = null,
            voice = voice,
            words = emptyList(),
            blank = text.isEmpty(),
            written = 0u,
            breathes = breathes,
        )

    private fun sheet(detail: LyricDetail = LyricDetail.WORDS) =
        LyricSheet(
            trackId = 7uL,
            origin = "LRCLIB",
            detail = detail,
            twoVoices = true,
            end = 8u,
            writtenEnd = 8u,
            lines =
                listOf(
                    row("One", 1_000uL),
                    row("Two", 2_000uL),
                    row("Three", 3_000uL, voice = 2u),
                    row("Four", 4_000uL),
                    row("", 5_000uL),
                    row("Six", 6_000uL, breathes = true),
                    row("Seven", 7_000uL),
                    row("Eight", 8_000uL),
                ),
        )

    private fun frame(
        readAt: UInt?,
        active: List<UInt> = emptyList(),
        wipes: List<LyricWipe> = emptyList(),
        waiting: LyricWait? = null,
    ) =
        LyricFrame(
            line = readAt,
            active = active,
            voiceOne = null,
            voiceTwo = null,
            readAt = readAt,
            reads = LyricReads.Evenly,
            waiting = waiting,
            ended = false,
            wipes = wipes,
        )

    @Test
    fun aWipeLightsTheSungLettersAndBlendsTheOneBeingSung() {
        val spans = wipeSpans(10, LyricWipe(0u, 4u, LyricLetter(4u, 5u, 0.4f)))

        assertEquals(WipeSpans(4, 4, 5, 0.4f), spans)
    }

    @Test
    fun aWipeIsHeldInsideItsLine() {
        assertEquals(WipeSpans(5, 5, 5, 0f), wipeSpans(5, LyricWipe(0u, 9u, LyricLetter(12u, 20u, 2f))))
        assertEquals(WipeSpans(3, 3, 3, 0f), wipeSpans(8, LyricWipe(0u, 3u, LyricLetter(1u, 2u, 0.5f))))
        assertEquals(WipeSpans(2, 2, 2, 0f), wipeSpans(8, LyricWipe(0u, 2u, null)))
        assertEquals(WipeSpans(0, 0, 3, 1f), wipeSpans(8, LyricWipe(0u, 0u, LyricLetter(0u, 3u, 7f))))
    }

    @Test
    fun aWipeCountsBothHalvesOfASurrogatePair() {
        val text = "a🎵b"

        val spans = wipeSpans(text.length, LyricWipe(0u, 1u, LyricLetter(1u, 3u, 0.5f)))

        assertEquals(4, text.length)
        assertEquals("🎵", text.substring(spans.blendStart, spans.blendEnd))
    }

    @Test
    fun theActiveLineIsLitWithItsWipeAndDistantLinesAreSoftened() {
        val reading =
            FrameReading(
                sheet(),
                frame(3u, active = listOf(3u), wipes = listOf(LyricWipe(3u, 2u, LyricLetter(2u, 3u, 0.5f)))),
                following = true,
            )

        assertEquals(3, reading.followed)
        assertEquals(LineLook(LineTone.LIT, WipeSpans(2, 2, 3, 0.5f), null), reading.look(3))
        assertEquals(LineTone.WAITING, reading.look(1).tone)
        assertEquals(LineTone.WAITING, reading.look(5).tone)
        assertEquals(LineTone.DISTANT, reading.look(0).tone)
        assertEquals(LineTone.DISTANT, reading.look(7).tone)
    }

    @Test
    fun aLitLineWithoutWordTimingIsLitWhole() {
        val reading = FrameReading(sheet(LyricDetail.LINES), frame(1u, active = listOf(1u)), following = true)

        assertEquals(LineLook(LineTone.LIT, null, null), reading.look(1))
    }

    @Test
    fun twoLinesSungTogetherAreBothLit() {
        val reading = FrameReading(sheet(), frame(2u, active = listOf(2u, 3u)), following = true)

        assertEquals(LineTone.LIT, reading.look(2).tone)
        assertEquals(LineTone.LIT, reading.look(3).tone)
        assertTrue(rightVoice(sheet().lines[2].voice))
        assertFalse(rightVoice(sheet().lines[3].voice))
    }

    @Test
    fun unsyncedLyricsArePlainAndNeitherFollowNorSeek() {
        val reading = FrameReading(sheet(LyricDetail.UNSYNCED), frame(3u, active = listOf(3u)), following = true)

        assertNull(reading.followed)
        assertTrue((0 until 8).all { reading.look(it).tone == LineTone.PLAIN })
        assertNull(reading.seekAt(3))
    }

    @Test
    fun aSongNotPlayingIsShownPlainWithoutSeeking() {
        val reading = FrameReading(sheet(), null, following = false)

        assertNull(reading.followed)
        assertEquals(LineLook(LineTone.PLAIN, null, null), reading.look(0))
        assertNull(reading.seekAt(0))
    }

    @Test
    fun aTimedLineSeeksToItsTime() {
        val reading = FrameReading(sheet(), frame(0u), following = true)

        assertEquals(3_000L, reading.seekAt(2))
        assertNull(reading.seekAt(42))
    }

    @Test
    fun theEndOfTheLyricsIsFollowedPastTheLastLine() {
        val reading = FrameReading(sheet(), frame(8u), following = true)

        assertEquals(8, reading.followed)
        assertEquals(8, FrameReading(sheet(), frame(30u), following = true).followed)
    }

    @Test
    fun aPauseBreathesBeforeTheNextLineWhereThereIsRoom() {
        val waiting = FrameReading(sheet(), frame(5u, waiting = LyricWait(5u, 0.6f)), following = true)
        val cramped = FrameReading(sheet(), frame(3u, waiting = LyricWait(3u, 0.6f)), following = true)

        assertEquals(0.6f, waiting.look(5).breath)
        assertNull(waiting.look(4).breath)
        assertNull(cramped.look(3).breath)
    }
}
