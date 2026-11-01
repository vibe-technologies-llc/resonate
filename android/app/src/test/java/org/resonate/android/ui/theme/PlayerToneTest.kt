package org.resonate.android.ui.theme

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.toArgb
import com.materialkolor.hct.Hct
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class PlayerToneTest {
    private val cover = Color(0xFF5B7FD6)

    private fun hct(color: Color) = Hct.fromInt(color.toArgb())

    @Test
    fun the_middle_intensity_is_the_colour_as_it_was() {
        assertEquals(cover, PlayerTone.top(cover, 0.5f, dark = true))
        assertEquals(cover, PlayerTone.top(cover, 0.5f, dark = false))
    }

    @Test
    fun a_lower_intensity_is_darker_and_duller() {
        listOf(true, false).forEach { dark ->
            val dimmed = hct(PlayerTone.top(cover, 0f, dark))
            assertTrue(dimmed.tone < hct(cover).tone)
            assertTrue(dimmed.chroma < hct(cover).chroma)
        }
    }

    @Test
    fun a_higher_intensity_is_brighter_and_richer() {
        listOf(true, false).forEach { dark ->
            val vivid = hct(PlayerTone.top(cover, 1f, dark))
            assertTrue(vivid.tone > hct(cover).tone)
            assertTrue(vivid.chroma > hct(cover).chroma)
        }
    }

    @Test
    fun the_dark_theme_dims_further_than_the_light_one() {
        val dark = hct(PlayerTone.top(cover, 0f, dark = true)).tone
        val light = hct(PlayerTone.top(cover, 0f, dark = false)).tone
        assertTrue(dark < light)
    }

    @Test
    fun an_intensity_outside_zero_to_one_is_held_to_it() {
        assertEquals(PlayerTone.top(cover, 1f, true), PlayerTone.top(cover, 9f, true))
        assertEquals(PlayerTone.top(cover, 0f, true), PlayerTone.top(cover, -9f, true))
    }
}
