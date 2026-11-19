package org.resonate.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class WidgetPressesTest {
    @Test
    fun aPlayDrawnWhileNothingPlaysStartsTheServiceAsAHeadsetsPlayDoes() {
        val drawnPaused = WidgetPresses.playStartsTheService(playing = false)
        val drawnPlaying = WidgetPresses.playStartsTheService(playing = true)

        assertTrue(drawnPaused)
        assertFalse(drawnPlaying)
    }

    @Test
    fun aPressReachingAServiceThatIsGoneStartsItToPlayAndNeverBindsOneOnlyToLetItGo() {
        val toggleCold = WidgetPresses.pressed(WidgetPresses.TOGGLE, serviceRunning = false)
        val nextCold = WidgetPresses.pressed(WidgetPresses.NEXT, serviceRunning = false)
        val previousCold = WidgetPresses.pressed(WidgetPresses.PREVIOUS, serviceRunning = false)

        assertEquals(WidgetPress.STARTED, toggleCold)
        assertEquals(WidgetPress.UNHEARD, nextCold)
        assertEquals(WidgetPress.UNHEARD, previousCold)
    }

    @Test
    fun aPressReachingARunningServiceGoesThroughAShortLivedController() {
        for (action in WidgetPresses.actions) {
            assertEquals(WidgetPress.CONTROLLED, WidgetPresses.pressed(action, serviceRunning = true))
        }
    }
}
