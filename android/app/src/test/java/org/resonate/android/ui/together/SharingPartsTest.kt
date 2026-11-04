package org.resonate.android.ui.together

import com.google.zxing.BinaryBitmap
import com.google.zxing.RGBLuminanceSource
import com.google.zxing.common.HybridBinarizer
import com.google.zxing.qrcode.QRCodeReader
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

private const val PIXELS_A_MODULE = 4
private const val WHITE = 0xFFFFFFFF.toInt()
private const val BLACK = 0xFF000000.toInt()

class SharingPartsTest {
    @Test
    fun aFirstPressArmsAndASecondOfTheSameKeyConfirms() {
        val first = pressed<String>(null, "remove")
        val again = pressed(first.armed, "remove")

        assertEquals("remove", first.armed)
        assertFalse(first.confirmed)
        assertNull(again.armed)
        assertTrue(again.confirmed)
    }

    @Test
    fun aPressOfAnotherKeyArmsThatOneAndConfirmsNothing() {
        val other = pressed("remove", "leave")

        assertEquals("leave", other.armed)
        assertFalse(other.confirmed)
    }

    @Test
    fun theModulesDrawnReadBackAsTheLink() {
        val link = "https://resonate.tecnio.dev/p/#ABCDE-FGHIJ-KLMNO-PQRST-UVWXY-Z2345"
        val modules = checkNotNull(QrModules.of(link))
        val across = (modules.side + 2 * QUIET_MODULES) * PIXELS_A_MODULE
        val pixels =
            IntArray(across * across) { at ->
                val column = at % across / PIXELS_A_MODULE - QUIET_MODULES
                val row = at / across / PIXELS_A_MODULE - QUIET_MODULES
                val within = column in 0 until modules.side && row in 0 until modules.side
                if (within && modules.isDark(column, row)) BLACK else WHITE
            }
        val read =
            QRCodeReader()
                .decode(BinaryBitmap(HybridBinarizer(RGBLuminanceSource(across, across, pixels))))

        assertEquals(link, read.text)
    }
}
