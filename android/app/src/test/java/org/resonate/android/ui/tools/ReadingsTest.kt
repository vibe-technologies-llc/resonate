package org.resonate.android.ui.tools

import kotlin.math.PI
import kotlin.math.sin
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.resonate.android.nativecore.AnalysisReport
import org.resonate.android.nativecore.Inspector
import org.resonate.android.nativecore.Track

class ReadingsTest {
    private fun report(
        loudness: Float? = -9.5f,
        dynamicRange: UInt? = 8u,
        bits: UInt? = 16u,
        clipped: ULong = 0uL,
    ) =
        AnalysisReport(
            trackId = 1uL,
            verdict = "genuine",
            peak = 1f,
            rms = 0.1f,
            truePeak = 1.12f,
            loudnessLufs = loudness,
            dynamicRange = dynamicRange,
            clipped = clipped,
            bitsInUse = bits,
            waveform = emptyList(),
            spectrum = emptyList(),
            spectrumStepHz = 0f,
            findings = emptyList(),
        )

    private val track =
        Track(
            id = 1uL,
            uri = "content://test/1",
            name = "01 Song.flac",
            title = "Song",
            artist = "Artist",
            album = "",
            durationMs = 185_000uL,
            rate = 96_000u,
            codec = "FLAC",
            favourite = false,
            albumArtist = "",
            genre = "Jazz",
            date = "",
            trackNumber = 3u,
            discNumber = null,
            bits = 24u,
            channels = 2u,
            recording = "8f1c",
            release = null,
            artistMbid = "",
            artRevision = 0uL,
            queueId = null,
        )

    @Test
    fun everyVerdictTheAnalysisGivesReadsAsAHeadline() {
        assertEquals(Tone.GOOD, verdictOf("genuine").tone)
        assertEquals("Consistent with lossless", verdictOf("genuine").title)
        assertEquals(Tone.WARNING, verdictOf("suspect").tone)
        assertEquals(Tone.BAD, verdictOf("fake").tone)
        assertEquals("Lossy by its codec", verdictOf("lossy").title)
        assertEquals("Not judged", verdictOf("not-judged").title)
        assertEquals(Tone.NEUTRAL, verdictOf("anything else").tone)
    }

    @Test
    fun levelsAreReadInDecibelsWithTheirSign() {
        val metrics = metricsOf(report()).associate { it.label to it.value }

        assertEquals("-9.5 LUFS", metrics["Loudness"])
        assertEquals("DR8", metrics["Dynamic range"])
        assertEquals("0.00 dBFS", metrics["Peak"])
        assertEquals("+0.98 dBTP", metrics["True peak"])
        assertEquals("-20.00 dBFS", metrics["RMS"])
        assertEquals("0", metrics["Clipped samples"])
        assertEquals("16 bit", metrics["Bits in use"])
    }

    @Test
    fun aMeasureTheAnalysisCouldNotTakeIsLeftOut() {
        val labels = metricsOf(report(loudness = null, dynamicRange = null, bits = null).copy(peak = 0f)).map { it.label }

        assertEquals(listOf("True peak", "RMS", "Clipped samples"), labels)
    }

    @Test
    fun clippedSamplesAreCountedReadably() {
        assertEquals("12,345", metricsOf(report(clipped = 12_345uL)).first { it.label == "Clipped samples" }.value)
    }

    @Test
    fun theSpectrumIsScaledFromItsFloorToFullScale() {
        assertEquals(listOf(1f, 0.5f, 0f, 0f, 0f), spectrumHeights(listOf(0f, -60f, -120f, -200f, Float.NEGATIVE_INFINITY)))
    }

    @Test
    fun theSpectrumIsMarkedEveryFiveKilohertzBelowItsTop() {
        val marks = spectrumMarks(count = 2048, stepHz = 11.71875f)

        assertEquals(listOf("5k", "10k", "15k", "20k"), marks.map { it.second })
        assertEquals(5_000f / (2048 * 11.71875f), marks.first().first, 0.0001f)
        assertEquals(listOf("10k", "20k", "30k", "40k"), spectrumMarks(2048, 23.4375f).map { it.second })
        assertTrue(spectrumMarks(1, 10f).isEmpty())
    }

    @Test
    fun theWaveformIsDrawnAgainstItsOwnPeak() {
        assertEquals(listOf(0.5f, 1f, 0.25f), waveformHeights(listOf(0.02f, -0.04f, 0.01f)))
        assertEquals(listOf(0f, 0f), waveformHeights(listOf(0f, 0f)))
    }

    @Test
    fun technicalDetailsLeaveOutWhatIsUnknown() {
        val shown = technicalDetails(track, null).associate { it.label to it.value }

        assertEquals("FLAC", shown["Codec"])
        assertEquals("96,000 Hz", shown["Sample rate"])
        assertEquals("24 bit", shown["Bit depth"])
        assertEquals("Stereo", shown["Channels"])
        assertEquals("3:05", shown["Duration"])
        assertFalse("Container" in shown)
        assertFalse("Seekable" in shown)
    }

    @Test
    fun theInspectorAddsTheContainerAndTheTagsItRead() {
        val inspector = Inspector(track, "FLAC", "S24", true, "", "© Label", "", "GBAYE0601498")

        val technical = technicalDetails(track, inspector).associate { it.label to it.value }
        val tagged = taggedDetails(track, inspector)

        assertEquals("FLAC", technical["Container"])
        assertEquals("Yes", technical["Seekable"])
        assertFalse("Encoder" in technical)
        assertEquals(
            listOf("Title", "Artist", "Genre", "Track", "ISRC", "MusicBrainz recording", "Copyright", "File"),
            tagged.map { it.label },
        )
        assertTrue(tagged.first { it.label == "ISRC" }.copyable)
        assertFalse(tagged.first { it.label == "Copyright" }.copyable)
    }

    @Test
    fun aToneSoundsInTheBandHoldingItsFrequency() {
        val bands = Bands(count = 32, window = 256)
        val tone = List(256) { sin(2 * PI * 40 * it / 256).toFloat() * 0.8f }

        val levels = bands.levels(tone)
        val loudest = levels.indices.maxBy { levels[it] }

        assertEquals(32, levels.size)
        assertTrue(levels[loudest] > 0.9f)
        assertTrue(levels.first() < 0.5f)
        assertEquals(List(32) { 0f }, bands.levels(List(256) { 0f }))
        assertEquals(List(32) { 0f }, bands.levels(emptyList()))
    }

    @Test
    fun barsRiseQuicklyAndFallSlowly() {
        val eased = eased(listOf(0f, 1f), listOf(1f, 0f))

        assertEquals(0.6f, eased[0], 0.0001f)
        assertEquals(0.82f, eased[1], 0.0001f)
        assertEquals(listOf(0.5f), eased(listOf(0f, 0f), listOf(0.5f)))
    }

    @Test
    fun loudnessIsTheRootMeanSquare() {
        assertEquals(0.5f, loudnessOf(listOf(0.5f, -0.5f)), 0.0001f)
        assertEquals(0f, loudnessOf(emptyList()))
    }

    @Test
    fun aRecognitionScoreIsAPercentage() {
        assertEquals("87% match", scoreOf(87u))
        assertEquals("100% match", scoreOf(140u))
    }
}
