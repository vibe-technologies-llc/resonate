package org.resonate.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class WidgetPressesTest {
    @Test
    fun aPlayDrawnWhileNothingPlaysStartsTheServiceAsAHeadsetsPlayDoes() {
        val drawnPaused =
            WidgetPresses.startsTheService(WidgetPresses.TOGGLE, playing = false, serviceRunning = true)
        val drawnPlaying =
            WidgetPresses.startsTheService(WidgetPresses.TOGGLE, playing = true, serviceRunning = true)

        assertTrue(drawnPaused)
        assertFalse(drawnPlaying)
    }

    @Test
    fun aSkipDrawnWithNoServiceStartsItAndOneDrawnBesideAPausedServiceOnlySkips() {
        for (action in listOf(WidgetPresses.PREVIOUS, WidgetPresses.NEXT)) {
            val drawnCold =
                WidgetPresses.startsTheService(action, playing = false, serviceRunning = false)
            val drawnPaused =
                WidgetPresses.startsTheService(action, playing = false, serviceRunning = true)
            val drawnPlaying =
                WidgetPresses.startsTheService(action, playing = true, serviceRunning = true)

            assertTrue(drawnCold)
            assertFalse(drawnPaused)
            assertFalse(drawnPlaying)
        }
    }

    @Test
    fun aPressReachingAServiceThatIsGoneStartsItAndNeverBindsOneOnlyToLetItGo() {
        val cold = WidgetPresses.pressed(serviceRunning = false)

        assertEquals(WidgetPress.STARTED, cold)
    }

    @Test
    fun aPressReachingARunningServiceGoesThroughAShortLivedController() {
        val warm = WidgetPresses.pressed(serviceRunning = true)

        assertEquals(WidgetPress.CONTROLLED, warm)
    }

    @Test
    fun theServiceHearsOnlyTheWidgetsSkipsAsColdSkips() {
        val previous = WidgetPresses.coldSkip(WidgetPresses.PREVIOUS)
        val next = WidgetPresses.coldSkip(WidgetPresses.NEXT)
        val toggle = WidgetPresses.coldSkip(WidgetPresses.TOGGLE)
        val none = WidgetPresses.coldSkip(null)

        assertEquals(ColdSkip.PREVIOUS, previous)
        assertEquals(ColdSkip.NEXT, next)
        assertNull(toggle)
        assertNull(none)
    }

    @Test
    fun aColdSkipResumesTheSavedQueueOnceAndASkipReachingAHeldQueueOnlySkips() {
        val cold = WidgetPresses.resumesBeforeSkipping(holdsARow = false, resumedAlready = false)
        val held = WidgetPresses.resumesBeforeSkipping(holdsARow = true, resumedAlready = false)
        val second = WidgetPresses.resumesBeforeSkipping(holdsARow = false, resumedAlready = true)

        assertTrue(cold)
        assertFalse(held)
        assertFalse(second)
    }
}
