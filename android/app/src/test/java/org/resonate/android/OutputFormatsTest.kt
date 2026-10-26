package org.resonate.android

import android.media.AudioFormat
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import org.resonate.android.nativecore.OutputEncoding
import org.resonate.android.nativecore.OutputFormat

class OutputFormatsTest {
    private fun output(id: Int) =
        AudioOutput(id, "device $id", "Audio device", emptyList(), emptyList(), emptyList())

    @Test
    fun anEncodingAStreamCanCarryIsNamedAndAnyOtherIsLeftOut() {
        assertEquals(OutputEncoding.PCM16, outputEncodingOf(AudioFormat.ENCODING_PCM_16BIT))
        assertEquals(OutputEncoding.PCM32, outputEncodingOf(AudioFormat.ENCODING_PCM_32BIT))
        assertEquals(OutputEncoding.FLOAT, outputEncodingOf(AudioFormat.ENCODING_PCM_FLOAT))
        assertNull(outputEncodingOf(AudioFormat.ENCODING_PCM_24BIT_PACKED))
        assertNull(outputEncodingOf(AudioFormat.ENCODING_AC3))
    }

    @Test
    fun aMixerFormatIsOfferedOnlyWithARateAndOneOrTwoChannels() {
        assertEquals(
            OutputFormat(96_000u, OutputEncoding.PCM32, 2u),
            outputFormatOf(AudioFormat.ENCODING_PCM_32BIT, 96_000, 2),
        )
        assertNull(outputFormatOf(AudioFormat.ENCODING_PCM_16BIT, 0, 2))
        assertNull(outputFormatOf(AudioFormat.ENCODING_PCM_16BIT, 44_100, 6))
        assertNull(outputFormatOf(AudioFormat.ENCODING_PCM_24BIT_PACKED, 44_100, 2))
    }

    @Test
    fun theDeviceInUseIsTheStreamsElseTheFirstMediaWouldTakeThatIsListed() {
        val outputs = listOf(output(3), output(7))

        assertEquals(7, deviceInUse(outputs, 7, listOf(3)))
        assertEquals(3, deviceInUse(outputs, 0, listOf(9, 3)))
        assertNull(deviceInUse(outputs, 0, listOf(9)))
    }
}
