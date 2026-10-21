package org.resonate.android.ui

import org.junit.Assert.assertEquals
import org.junit.Test

class WordsTest {
    @Test
    fun aDownwardDragLandsBeforeTheRowAfterItsPlace() {
        assertEquals(4, queueLanding(1, 3))
        assertEquals(1, queueLanding(3, 1))
        assertEquals(2, queueLanding(2, 2))
    }

    @Test
    fun clocksShowHoursOnlyWhenThereAreAny() {
        assertEquals("0:00", clock(-5L))
        assertEquals("3:07", clock(187_000L))
        assertEquals("1:02:03", clock(3_723_000L))
    }

    @Test
    fun lengthsReadAsMinutesAndHours() {
        assertEquals("under a minute", length(59_000uL))
        assertEquals("42 min", length(2_520_000uL))
        assertEquals("2 hr 5 min", length(7_500_000uL))
    }

    @Test
    fun ratesReadInKilohertz() {
        assertEquals("44.1 kHz", kilohertz(44_100u))
        assertEquals("96 kHz", kilohertz(96_000u))
    }

    @Test
    fun countsTakeTheirPlural() {
        assertEquals("1 song", counted(1, "song"))
        assertEquals("3 songs", counted(3, "song"))
        assertEquals("2 libraries", counted(2, "library", "libraries"))
    }

    @Test
    fun enumNamesReadAsWords() {
        assertEquals("Very high", humanized("VERY_HIGH"))
    }
}
