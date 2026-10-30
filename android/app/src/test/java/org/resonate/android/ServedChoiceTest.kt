package org.resonate.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import org.resonate.android.nativecore.ServedService

class ServedChoiceTest {
    private val subsonic = ServedService("subsonic", "Subsonic", true)
    private val tidal = ServedService("tidal", "TIDAL", true)
    private val monochrome = ServedService("monochrome", "Monochrome", false)

    @Test
    fun nothingIsChosenWhereNothingIsServed() {
        assertNull(chosenService(emptyList(), null))
        assertNull(chosenService(emptyList(), "tidal"))
    }

    @Test
    fun theFirstServiceIsChosenUntilAnotherIsPicked() {
        assertEquals(subsonic, chosenService(listOf(subsonic, tidal), null))
        assertEquals(tidal, chosenService(listOf(subsonic, tidal), "tidal"))
    }

    @Test
    fun aPickStandsWhileItsServiceIsStillServed() {
        val before = listOf(subsonic, tidal, monochrome)
        val after = listOf(tidal, monochrome)

        assertEquals(monochrome, chosenService(before, "monochrome"))
        assertEquals(monochrome, chosenService(after, "monochrome"))
    }

    @Test
    fun aPickWhoseServiceWentFallsToTheFirst() {
        assertEquals(subsonic, chosenService(listOf(subsonic, monochrome), "tidal"))
    }
}
