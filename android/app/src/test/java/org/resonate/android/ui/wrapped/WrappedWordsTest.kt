package org.resonate.android.ui.wrapped

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.resonate.android.English

class WrappedWordsTest {
    @Test
    fun aSlideSaysWhereItStandsCountedFromOne() {
        assertEquals("1 of 9", WrappedWords.place(English, 0, 9))
        assertEquals("9 of 9", WrappedWords.place(English, 8, 9))
    }

    @Test
    fun aBarsShareIsAFractionHeldToTheWhole() {
        assertEquals(0f, WrappedWords.barFraction(0u))
        assertEquals(0.5f, WrappedWords.barFraction(500u))
        assertEquals(1f, WrappedWords.barFraction(1_000u))
        assertEquals(1f, WrappedWords.barFraction(1_500u))
    }

    @Test
    fun theDaysAreEachLabelledTheMonthsEveryThirdAndTheHoursEverySixth() {
        assertTrue((0 until 7).all { WrappedWords.labelsBar(it, 7) })
        assertEquals(listOf(0, 3, 6, 9), (0 until 12).filter { WrappedWords.labelsBar(it, 12) })
        assertEquals(listOf(0, 6, 12, 18), (0 until 24).filter { WrappedWords.labelsBar(it, 24) })
        assertFalse(WrappedWords.labelsBar(1, 24))
    }

    @Test
    fun aCardIsNamedByItsYearAndSlide() {
        assertEquals("resonate-2026-3.png", WrappedWords.cardFileName(2026, 2))
    }
}
