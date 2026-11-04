package org.resonate.android

import android.media.AudioDeviceInfo
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.resonate.android.ui.settings.standsFor

class OutputRoutesTest {
    private fun output(id: Int, route: OutputRoute?) =
        AudioOutput(id, "device $id", "Audio device", emptyList(), emptyList(), emptyList(), route)

    @Test
    fun aBluetoothHeadsetIsTheSameRouteWhateverProfileOrIdItComesBackWith() {
        val media = outputRouteOf(AudioDeviceInfo.TYPE_BLUETOOTH_A2DP, "AA:BB:CC:DD:EE:FF", "Buds", "Buds")
        val call = outputRouteOf(AudioDeviceInfo.TYPE_BLUETOOTH_SCO, "AA:BB:CC:DD:EE:FF", "Buds", "Buds")
        val renamed = outputRouteOf(AudioDeviceInfo.TYPE_BLE_HEADSET, "AA:BB:CC:DD:EE:FF", "My buds", "My buds")

        assertEquals(media.id, call.id)
        assertEquals(media.id, renamed.id)
    }

    @Test
    fun twoBluetoothDevicesAreTwoRoutes() {
        val first = outputRouteOf(AudioDeviceInfo.TYPE_BLUETOOTH_A2DP, "AA:BB:CC:DD:EE:01", "Buds", "Buds")
        val second = outputRouteOf(AudioDeviceInfo.TYPE_BLUETOOTH_A2DP, "AA:BB:CC:DD:EE:02", "Buds", "Buds")

        assertNotEquals(first.id, second.id)
    }

    @Test
    fun aUsbDacIsKnownByItsNameNotByTheCardNumberAndroidGaveIt() {
        val first = outputRouteOf(AudioDeviceInfo.TYPE_USB_DEVICE, "card=1;device=0", "FiiO K5", "FiiO K5")
        val again = outputRouteOf(AudioDeviceInfo.TYPE_USB_DEVICE, "card=2;device=0", "FiiO K5", "FiiO K5")
        val other = outputRouteOf(AudioDeviceInfo.TYPE_USB_HEADSET, "card=2;device=0", "Dongle", "Dongle")

        assertEquals(first.id, again.id)
        assertNotEquals(first.id, other.id)
    }

    @Test
    fun theSpeakerAndTheWiredJackAreDifferentRoutes() {
        val speaker = outputRouteOf(AudioDeviceInfo.TYPE_BUILTIN_SPEAKER, "", "Pixel", "Built-in speaker")
        val safe = outputRouteOf(AudioDeviceInfo.TYPE_BUILTIN_SPEAKER_SAFE, "", "Pixel", "Built-in speaker")
        val jack = outputRouteOf(AudioDeviceInfo.TYPE_WIRED_HEADPHONES, "", "Pixel", "Wired headphones")

        assertEquals(speaker.id, safe.id)
        assertNotEquals(speaker.id, jack.id)
    }

    @Test
    fun aRouteIdBeginsWithTheFormItWasKeptIn() {
        val route = outputRouteOf(AudioDeviceInfo.TYPE_HDMI, "", "TV", "TV")

        assertEquals("v1:hdmi:TV", route.id)
    }

    @Test
    fun theStreamsDeviceDecidesTheRouteAndAndroidsChoiceStandsInBeforeOneOpens() {
        val speaker = OutputRoute("v1:speaker:Pixel", "Built-in speaker")
        val buds = OutputRoute("v1:bluetooth:AA", "Buds")
        val outputs = listOf(output(2, speaker), output(7, buds))

        assertEquals(buds, routeInUse(outputs, 7, listOf(2)))
        assertEquals(speaker, routeInUse(outputs, 0, listOf(2)))
        assertEquals(speaker, routeInUse(outputs, 99, listOf(2)))
        assertNull(routeInUse(outputs, 0, emptyList()))
    }

    private val dac = OutputRoute("v1:usb:Topping", "Topping")

    private val headset = OutputRoute("v1:bluetooth:AA:BB", "Headset")

    private fun twin(id: Int, route: OutputRoute, type: Int, rates: List<Int> = emptyList()) =
        AudioOutput(id, route.name, "Audio device", rates, emptyList(), emptyList(), route, type)

    @Test
    fun aDacAndroidListsAsTwoDevicesIsOfferedOnceWhereItFirstStood() {
        val speaker = output(1, OutputRoute("v1:speaker:Phone", "Built-in speaker"))
        val listed =
            listOf(
                twin(30, dac, AudioDeviceInfo.TYPE_USB_DEVICE),
                speaker,
                twin(31, dac, AudioDeviceInfo.TYPE_USB_HEADSET),
            )

        assertEquals(listOf(30, 1), oneForEachRoute(listed).map { it.id })
    }

    @Test
    fun ofTwinsTheOneTakingTheMostRatesIsOffered() {
        val listed =
            listOf(
                twin(30, dac, AudioDeviceInfo.TYPE_USB_HEADSET, listOf(48_000)),
                twin(31, dac, AudioDeviceInfo.TYPE_USB_DEVICE, listOf(44_100, 48_000, 96_000)),
            )

        assertEquals(listOf(31), oneForEachRoute(listed).map { it.id })
    }

    @Test
    fun theProfileForCallsIsNeverOfferedBesideTheOneForMusicWhicheverComesFirstOrTakesMore() {
        val calls = twin(40, headset, AudioDeviceInfo.TYPE_BLUETOOTH_SCO, listOf(8_000, 16_000))
        val music = twin(41, headset, AudioDeviceInfo.TYPE_BLUETOOTH_A2DP, listOf(48_000))

        assertEquals(listOf(41), oneForEachRoute(listOf(calls, music)).map { it.id })
        assertEquals(listOf(41), oneForEachRoute(listOf(music, calls)).map { it.id })
    }

    @Test
    fun aRouteKeptIsBoundToTheTwinOffered() {
        val calls = twin(40, headset, AudioDeviceInfo.TYPE_BLUETOOTH_SCO)
        val music = twin(41, headset, AudioDeviceInfo.TYPE_BLUETOOTH_A2DP)

        assertEquals(41, ZonePlan.deviceFor(headset.id, listOf(calls, music)))
    }

    @Test
    fun outputsOnOtherRoutesOrOnNoneAreAllOffered() {
        val other = OutputRoute("v1:usb:FiiO", "FiiO")
        val listed =
            listOf(
                twin(30, dac, AudioDeviceInfo.TYPE_USB_DEVICE),
                twin(31, other, AudioDeviceInfo.TYPE_USB_DEVICE),
                output(32, null),
                output(33, null),
            )

        assertEquals(listOf(30, 31, 32, 33), oneForEachRoute(listed).map { it.id })
    }

    @Test
    fun anOutputOfferedStandsForTheTwinItWasNotChosenOver() {
        val offered = twin(30, dac, AudioDeviceInfo.TYPE_USB_DEVICE)
        val other = twin(31, dac, AudioDeviceInfo.TYPE_USB_HEADSET)
        val elsewhere = output(32, OutputRoute("v1:hdmi:TV", "TV"))
        val listed = listOf(offered, other, elsewhere)

        assertTrue(standsFor(offered, 31, listed))
        assertTrue(standsFor(offered, 30, listed))
        assertFalse(standsFor(offered, 32, listed))
        assertFalse(standsFor(output(33, null), 34, listed))
    }
}
