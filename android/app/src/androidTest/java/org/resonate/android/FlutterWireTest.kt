package org.resonate.android

import io.flutter.plugin.common.StandardMessageCodec
import org.junit.Assert.assertEquals
import org.junit.Test
import org.resonate.android.nativecore.HandedOff
import org.resonate.android.nativecore.LyricReads
import org.resonate.android.nativecore.SleepTimer
import org.resonate.android.nativecore.SyncConfig
import org.resonate.android.nativecore.defaultPlaybackPreferences

class FlutterWireTest {
    @Test
    fun unsignedIdentitySurvivesTheFlutterCodecWithoutRounding() {
        val encoded = flutterValue(mapOf("id" to ULong.MAX_VALUE, "rows" to listOf(1u, 2u)))
        val buffer = requireNotNull(StandardMessageCodec.INSTANCE.encodeMessage(encoded))
        buffer.flip()
        val decoded = StandardMessageCodec.INSTANCE.decodeMessage(buffer) as Map<*, *>
        assertEquals("18446744073709551615", decoded["id"])
        assertEquals(listOf(1L, 2L), decoded["rows"])
    }

    @Test
    fun persistedSettingsAndTaggedUnionsHaveExplicitWireValues() {
        val preferences = flutterValue(defaultPlaybackPreferences()) as Map<*, *>
        assertEquals(false, preferences["equaliserEnabled"])
        assertEquals(
            mapOf("kind" to "after", "seconds" to 1800L),
            flutterValue(SleepTimer.After(1800u)),
        )
        assertEquals(mapOf("kind" to "at", "line" to 4L), flutterValue(LyricReads.At(4u)))
        assertEquals(mapOf("kind" to "evenly"), flutterValue(LyricReads.Evenly))
    }

    @Test
    fun nearbySyncAndDeviceIdentitiesSurviveTheFlutterBridge() {
        val config = SyncConfig(true, true, "", "", "Phone", false, false, false, false)
        assertEquals(true, (flutterValue(config) as Map<*, *>)["nearbyOnly"])
        val handoff = HandedOff("device-id", "Phone", 2u, false)
        assertEquals("device-id", (flutterValue(handoff) as Map<*, *>)["id"])
    }
}
