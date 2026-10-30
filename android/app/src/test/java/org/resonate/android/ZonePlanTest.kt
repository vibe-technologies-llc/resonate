package org.resonate.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.resonate.android.nativecore.Transport

class ZonePlanTest {
    private val dac =
        AudioOutput(14, "Topping", "USB audio", emptyList(), emptyList(), emptyList(), OutputRoute("v1:usb:Topping", "Topping"))
    private val speaker =
        AudioOutput(
            2,
            "Pixel",
            "Built-in speaker",
            emptyList(),
            emptyList(),
            emptyList(),
            OutputRoute("v1:speaker:Pixel", "Built-in speaker"),
        )

    @Test
    fun aRouteKeptIsFoundAmongTheOutputsByItsNameNotItsPassingId() {
        val outputs = listOf(speaker, dac.copy(id = 31))

        assertEquals(31, ZonePlan.deviceFor("v1:usb:Topping", outputs))
        assertEquals(0, ZonePlan.deviceFor("", outputs))
        assertNull(ZonePlan.deviceFor("v1:hdmi:TV", outputs))
    }

    @Test
    fun theRouteOfADeviceIsItsNameAndTheSystemDefaultHasNone() {
        val outputs = listOf(speaker, dac)

        assertEquals("v1:usb:Topping", ZonePlan.routeOf(14, outputs))
        assertEquals("", ZonePlan.routeOf(0, outputs))
    }

    @Test
    fun aZoneNamedAfterAnOutputAlreadyNamingOneIsNumbered() {
        assertEquals("Topping", ZonePlan.nameFor("Topping", listOf("Zone 1")))
        assertEquals("Topping (2)", ZonePlan.nameFor("Topping", listOf("Zone 1", "Topping")))
        assertEquals("Topping (3)", ZonePlan.nameFor("Topping", listOf("Topping", "Topping (2)")))
    }

    @Test
    fun aZoneOnARendererIsHeardOnItAndOtherwiseOnItsDevice() {
        assertEquals("Living room streamer", ZonePlan.heardOn("Living room streamer", "Topping"))
        assertEquals("Topping", ZonePlan.heardOn(null, "Topping"))
    }

    @Test
    fun theSongIsHandedOnlyToAnotherZoneAndOnlyWhenTheShownOneHoldsAQueue() {
        assertTrue(ZonePlan.mayHandOverTo(2u, 1u, shownHolds = true))
        assertFalse(ZonePlan.mayHandOverTo(1u, 1u, shownHolds = true))
        assertFalse(ZonePlan.mayHandOverTo(2u, 1u, shownHolds = false))
        assertFalse(ZonePlan.mayRemove(FIRST_ZONE))
        assertTrue(ZonePlan.mayRemove(2u))
    }

    @Test
    fun theZonesAreReadAgainWhenTheyOrTheZoneShownMoveAndNowAndThenWhileThereAreSeveral() {
        val read = ZonePlan.Read(1u, 3uL)

        assertTrue(ZonePlan.due(null, 1u, 3uL, zones = 0, ticks = 1))
        assertFalse(ZonePlan.due(read, 1u, 3uL, zones = 1, ticks = 50))
        assertTrue(ZonePlan.due(read, 2u, 3uL, zones = 1, ticks = 1))
        assertTrue(ZonePlan.due(read, 1u, 4uL, zones = 1, ticks = 1))
        assertFalse(ZonePlan.due(read, 1u, 3uL, zones = 2, ticks = 7))
        assertTrue(ZonePlan.due(read, 1u, 3uL, zones = 2, ticks = 8))
    }

    @Test
    fun aZoneSaysItsOutputAndWhatItIsDoing() {
        assertEquals("Topping · Playing Echoes", ZonePlan.line("Topping", Transport.PLAYING, "Echoes"))
        assertEquals("Topping · Paused", ZonePlan.line("Topping", Transport.PAUSED, null))
        assertEquals("System default · Stopped", ZonePlan.line("System default", Transport.STOPPED, null))
    }
}
