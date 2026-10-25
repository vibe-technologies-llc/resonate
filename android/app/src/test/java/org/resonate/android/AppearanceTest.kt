package org.resonate.android

import java.io.File
import androidx.compose.ui.graphics.Color
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.resonate.android.ui.theme.schemeOf
import org.junit.Test

class AppearanceTest {
    private val palette =
        generateSequence(File("").absoluteFile) { it.parentFile }
            .map { File(it, "crates/resonate-ui/src/theme.rs") }
            .first { it.exists() }
            .readText()
            .substringAfter("static RESONATE")
            .substringBefore("static MIDNIGHT")

    @Test
    fun every_accent_is_the_resonate_palettes() {
        Accent.entries.forEach { accent ->
            val written = Regex("""\b${accent.name.lowercase()}: 0x([0-9a-f]{6})""").find(palette)
            assertEquals(accent.name, written?.groupValues?.get(1)?.toInt(16), accent.hex)
        }
    }

    @Test
    fun the_accents_are_the_palettes_seven() {
        assertEquals(7, Regex("""0x[0-9a-f]{6},\n""").findAll(palette.substringAfter("accents:")).count())
        assertEquals(7, Accent.entries.size)
    }

    @Test
    fun wallpaper_is_named_when_no_accent_is_chosen() {
        assertEquals("Wallpaper", Appearance().accentLabel)
        assertEquals("Teal", Appearance(accent = Accent.TEAL).accentLabel)
    }

    @Test
    fun pitch_black_is_off_until_chosen() {
        assertEquals(false, Appearance().pitchBlack)
        assertEquals(true, Appearance(pitchBlack = true).pitchBlack)
    }

    @Test
    fun a_pitch_black_scheme_has_a_black_surface_and_a_plain_one_does_not() {
        val seed = Color(0xFF7CB6EA)
        assertEquals(Color.Black, schemeOf(seed, dark = true, pitchBlack = true).surface)
        assertNotEquals(Color.Black, schemeOf(seed, dark = true).surface)
    }
}
