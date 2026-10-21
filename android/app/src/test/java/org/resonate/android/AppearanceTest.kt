package org.resonate.android

import java.io.File
import org.junit.Assert.assertEquals
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
}
