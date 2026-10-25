package org.resonate.android

import android.media.AudioDeviceInfo
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Test

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
}
