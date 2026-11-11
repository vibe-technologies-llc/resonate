package org.resonate.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class PollPaceTest {
    @Test
    fun aVisiblePlayerPlayingOrWorkingIsReadFourTimesASecond() {
        assertEquals(PollPace.BUSY_MS, PollPace.intervalMs(true, true, false, false))
        assertEquals(PollPace.BUSY_MS, PollPace.intervalMs(true, false, true, false))
    }

    @Test
    fun aVisiblePlayerAtRestIsReadOnceASecondAndABackgroundOneEveryTwo() {
        assertEquals(PollPace.IDLE_MS, PollPace.intervalMs(true, false, false, false))
        assertEquals(PollPace.BACKGROUND_MS, PollPace.intervalMs(false, true, false, false))
        assertEquals(PollPace.BACKGROUND_MS, PollPace.intervalMs(false, false, true, false))
    }

    @Test
    fun aRecentAskIsAnsweredAtTheBusyPaceWhereverTheAppIs() {
        assertEquals(PollPace.BUSY_MS, PollPace.intervalMs(false, false, false, true))
        assertEquals(PollPace.BUSY_MS, PollPace.intervalMs(true, false, false, true))
    }

    @Test
    fun anActionAgainstAnEngineThatClosedIsNotRun() {
        val engines = OpenEngine<String>()
        val ran = mutableListOf<String>()

        engines.opened("first")
        assertTrue(engines.run("first") { ran += it })

        engines.closing("first")
        assertFalse(engines.run("first") { ran += it })

        engines.opened("second")
        assertFalse(engines.run("first") { ran += it })
        assertTrue(engines.run("second") { ran += it })
        engines.closing("first")
        assertTrue(engines.run("second") { ran += it })
        assertEquals(listOf("first", "second", "second"), ran)
    }

    @Test
    fun anEngineHeldAsItClosesIsReleasedOnlyOnceItsLastHolderLetsGo() {
        val released = mutableListOf<String>()
        val engines = OpenEngine<String> { released += it }
        engines.opened("first")

        val read =
            engines.holding("first") { held ->
                engines.closed(held)
                assertTrue(released.isEmpty())
                assertFalse(engines.run(held) {})
                assertThrows(EngineClosed::class.java) { engines.holding(held) {} }
                assertTrue(released.isEmpty())
                "read"
            }

        assertEquals("read", read)
        assertEquals(listOf("first"), released)
    }

    @Test
    fun anEngineNobodyHoldsIsReleasedAsItClosesAndNeverAgain() {
        val released = mutableListOf<String>()
        val engines = OpenEngine<String> { released += it }
        engines.opened("first")
        engines.run("first") {}

        engines.closed("first")
        assertThrows(EngineClosed::class.java) { engines.holding("first") {} }

        assertEquals(listOf("first"), released)
    }
}
