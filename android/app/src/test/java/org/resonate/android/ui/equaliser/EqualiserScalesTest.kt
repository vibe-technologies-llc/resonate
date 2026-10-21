package org.resonate.android.ui.equaliser

import kotlin.math.ln
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.resonate.android.nativecore.EqualiserBand
import org.resonate.android.nativecore.EqualiserCurve
import org.resonate.android.nativecore.EqualiserFilter
import org.resonate.android.nativecore.RoomCorrection

class EqualiserScalesTest {
    private val imported =
        EqualiserCurve(
            "My headphones",
            -4f,
            listOf(
                EqualiserBand(EqualiserFilter.LOW_SHELF, 100f, 3f, 0.7f),
                EqualiserBand(EqualiserFilter.NOTCH, 2000f, 0f, 4f),
            ),
        )

    private val frame = GraphFrame(width = 360f, height = 240f, span = 15f)

    private val bounds = EqualiserBounds.unread

    private fun at(hertz: Float, decibels: Float) =
        GraphPoint(GraphScale.x(hertz, frame.width), GraphScale.y(decibels, frame.height, frame.span))

    @Test
    fun theFrequencyAxisIsThreeDecadesFromTwentyHertz() {
        assertEquals(0f, GraphScale.x(20f, 300f), 0.001f)
        assertEquals(100f, GraphScale.x(200f, 300f), 0.001f)
        assertEquals(200f, GraphScale.x(2000f, 300f), 0.001f)
        assertEquals(300f, GraphScale.x(20_000f, 300f), 0.001f)
        assertEquals(0f, GraphScale.x(5f, 300f), 0.001f)
        assertEquals(2000f, GraphScale.hertzAt(200f, 300f), 0.5f)
        assertEquals(20_000f, GraphScale.hertzAt(900f, 300f), 0.5f)
        listOf(31.25f, 440f, 9_000f).forEach { hertz ->
            assertEquals(hertz, GraphScale.hertzAt(GraphScale.x(hertz, 300f), 300f), hertz * 0.0001f)
        }
    }

    @Test
    fun theLevelAxisCentresZeroAndHoldsToTheSpan() {
        assertEquals(120f, GraphScale.y(0f, 240f, 15f), 0.001f)
        assertEquals(0f, GraphScale.y(15f, 240f, 15f), 0.001f)
        assertEquals(240f, GraphScale.y(-40f, 240f, 15f), 0.001f)
        assertEquals(7.5f, GraphScale.dbAt(GraphScale.y(7.5f, 240f, 15f), 240f, 15f), 0.001f)
    }

    @Test
    fun theSpanFitsTheLoudestPointInFiveDecibelStepsBetweenFifteenAndThirty() {
        assertEquals(15f, GraphScale.span(listOf(1f, -3f), imported))
        assertEquals(20f, GraphScale.span(listOf(16.2f), imported))
        assertEquals(30f, GraphScale.span(listOf(-80f), imported))
        assertEquals(15f, GraphScale.span(listOf(Float.NaN), imported))
        assertEquals(listOf(-15f, -10f, -5f, 0f, 5f, 10f, 15f), GraphScale.marks(15f))
    }

    @Test
    fun theResponseIsDrawnAcrossTheWholeWidth() {
        assertEquals(0f, GraphScale.responseX(0, 256, 300f))
        assertEquals(300f, GraphScale.responseX(255, 256, 300f))
    }

    @Test
    fun aHandleDraggedUpAndRightMovesItsBandAndKeepsItsFilterAndQ() {
        val start = at(100f, -1f)

        val held = frame.nearest(imported, start, reach = 32f)
        val moved = frame.placed(imported, 0, GraphPoint(start.x + 80f, start.y - 40f), bounds)

        assertEquals(0, held)
        assertEquals("", moved.name)
        assertEquals(2, moved.bands.size)
        assertTrue(moved.bands[0].frequencyHz > 100f)
        assertTrue(moved.bands[0].gainDb > 3f)
        assertEquals(EqualiserFilter.LOW_SHELF, moved.bands[0].filter)
        assertEquals(0.7f, moved.bands[0].q)
        assertEquals(imported.bands[1], moved.bands[1])
    }

    @Test
    fun aGainlessBandMovesAlongTheGraphAndKeepsItsGain() {
        val start = at(2000f, -4f)

        val moved = frame.placed(imported, 1, GraphPoint(start.x + 60f, start.y - 40f), bounds)

        assertEquals(1, frame.nearest(imported, start, reach = 32f))
        assertTrue(moved.bands[1].frequencyHz > 2000f)
        assertEquals(0f, moved.bands[1].gainDb)
        assertEquals(imported.bands[0], moved.bands[0])
    }

    @Test
    fun aDragBeginningAwayFromEveryHandleHoldsNone() {
        assertNull(frame.nearest(imported, at(8000f, 10f), reach = 32f))
    }

    @Test
    fun aPressOnAHandleFindsItsBand() {
        assertEquals(1, frame.nearest(imported, at(2000f, -4f), reach = 32f))
    }

    @Test
    fun aPlacedBandStaysWithinTheBounds() {
        val moved = frame.placed(imported, 0, GraphPoint(-500f, -500f), bounds)

        assertEquals(20f, moved.bands[0].frequencyHz)
        assertEquals(24f, moved.bands[0].gainDb)
    }

    @Test
    fun aTapAddsWhereItLands() {
        val (hertz, gain) = frame.added(imported, at(1000f, 2f))

        assertEquals(1000f, hertz, 1f)
        assertEquals(6f, gain, 0.01f)
    }

    @Test
    fun theLogSliderMapsItsEndsAndInvertsItself() {
        val scale = LogScale(20f, 20_000f)

        assertEquals(0f, scale.fraction(20f), 0.0001f)
        assertEquals(1f, scale.fraction(20_000f), 0.0001f)
        assertEquals(0.5f, scale.fraction(632.4555f), 0.0001f)
        assertEquals(20f, scale.actual(-1f), 0.001f)
        assertEquals(20_000f, scale.actual(2f), 0.5f)
        listOf(0.1f, 0.7f, 3f, 39f).forEach { q ->
            val narrow = LogScale(0.1f, 40f)
            assertEquals(q, narrow.actual(narrow.fraction(q)), q * 0.0001f)
        }
        assertEquals((ln(10.0) / ln(400.0)).toFloat(), LogScale(0.1f, 40f).fraction(1f), 0.0001f)
    }

    @Test
    fun aLogSliderWithAnEmptyRangeStillAnswers() {
        val scale = LogScale(0f, 0f)

        assertTrue(scale.fraction(5f).isFinite())
        assertTrue(scale.actual(0.5f).isFinite())
    }

    @Test
    fun aRoomResponseIsDescribedByChannelsRateAndLength() {
        assertEquals("Stereo · 48 kHz · 1.50 s", describeRoom(RoomCorrection(2u, 48_000u, 1_500uL)))
        assertEquals("Mono · 44.1 kHz · 0.25 s", describeRoom(RoomCorrection(1u, 44_100u, 250uL)))
        assertEquals("6 channels · 96 kHz · 10.00 s", describeRoom(RoomCorrection(6u, 96_000u, 10_000uL)))
        assertEquals("", describeRoom(RoomCorrection(0u, 48_000u, 1_000uL)))
        assertEquals("", describeRoom(null))
    }

    @Test
    fun anAutoEqAnswerNamesTheHeadphoneAndWhereItWasMeasured() {
        assertEquals(
            Measured("Sennheiser HD 650", "oratory1990"),
            measured("Sennheiser HD 650 — oratory1990"),
        )
        assertEquals(
            Measured("Sennheiser HD 650", "Crinacle on GRAS 43AG-7"),
            measured("Sennheiser HD 650 — Crinacle on GRAS 43AG-7"),
        )
        assertEquals(Measured("Unknown rig", null), measured("Unknown rig"))
    }

    @Test
    fun verticalSlidersAreUsedOnlyWhereEveryColumnHasRoom() {
        assertTrue(bankFits(10, 400f, 24f))
        assertFalse(bankFits(10, 300f, 24f))
        assertFalse(bankFits(10, 400f, 48f))
        assertFalse(bankFits(0, 400f, 10f))
        assertTrue(bankFits(2, 120f, 50f))
    }
}
