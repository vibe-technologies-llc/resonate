package org.resonate.android.ui.theme

import androidx.compose.material3.ColorScheme
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.compositeOver
import androidx.compose.ui.graphics.luminance
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.resonate.android.Accent
import org.resonate.android.ui.components.bannerTrackOf
import org.resonate.android.ui.components.titleLinesAt
import org.resonate.android.ui.tools.QUIET_ALPHA

class ThemeContrastTest {
    private class Variant(val name: String, val scheme: ColorScheme)

    private class Check(val name: String, val ink: (ColorScheme) -> Color, val ground: (ColorScheme) -> Color)

    private val wallpaperStandIn = Color(0xFF6750A4)

    private fun variants(contrast: Double): List<Variant> {
        val seeds =
            Accent.entries.map { it.name to Color(0xFF000000.toInt() or it.hex) } +
                ("Wallpaper" to wallpaperStandIn)
        return seeds.flatMap { (name, seed) ->
            listOf(
                Variant("$name light", schemeOf(seed, dark = false, contrastLevel = contrast)),
                Variant("$name dark", schemeOf(seed, dark = true, contrastLevel = contrast)),
                Variant(
                    "$name pitch black",
                    schemeOf(seed, dark = true, pitchBlack = true, contrastLevel = contrast),
                ),
            )
        }
    }

    private fun ratio(a: Color, b: Color): Double {
        val x = a.luminance().toDouble()
        val y = b.luminance().toDouble()
        return (maxOf(x, y) + 0.05) / (minOf(x, y) + 0.05)
    }

    private fun failures(
        contrast: Double,
        pairs: List<Check>,
        floor: Double,
    ): List<String> =
        variants(contrast).flatMap { variant ->
            pairs.mapNotNull { pair ->
                val found = ratio(pair.ink(variant.scheme), pair.ground(variant.scheme))
                if (found < floor) "${variant.name}: ${pair.name} ${"%.2f".format(found)} < $floor"
                else null
            }
        }

    private val textPairs =
        listOf(
            Check("onSurface on surface", { it.onSurface }, { it.surface }),
            Check("onSurfaceVariant on surface", { it.onSurfaceVariant }, { it.surface }),
            Check("onSurfaceVariant on surfaceContainerLow", { it.onSurfaceVariant }, { it.surfaceContainerLow }),
            Check("onSurface on surfaceContainerHighest", { it.onSurface }, { it.surfaceContainerHighest }),
            Check("onSurfaceVariant on surfaceContainerHighest", { it.onSurfaceVariant }, { it.surfaceContainerHighest }),
            Check("primary on surface", { it.primary }, { it.surface }),
            Check("primary on surfaceContainerLow", { it.primary }, { it.surfaceContainerLow }),
            Check("primary on surfaceContainerHighest", { it.primary }, { it.surfaceContainerHighest }),
            Check("onPrimary on primary", { it.onPrimary }, { it.primary }),
            Check("onPrimaryContainer on primaryContainer", { it.onPrimaryContainer }, { it.primaryContainer }),
            Check("onSecondaryContainer on secondaryContainer", { it.onSecondaryContainer }, { it.secondaryContainer }),
            Check("onTertiaryContainer on tertiaryContainer", { it.onTertiaryContainer }, { it.tertiaryContainer }),
            Check("onErrorContainer on errorContainer", { it.onErrorContainer }, { it.errorContainer }),
            Check("error on surface", { it.error }, { it.surface }),
            Check("inverseOnSurface on inverseSurface", { it.inverseOnSurface }, { it.inverseSurface }),
        )

    private val nonTextPairs =
        listOf(
            Check("inactive track (outline) on surface", { it.outline }, { it.surface }),
            Check("inactive track (outline) on surfaceContainerLow", { it.outline }, { it.surfaceContainerLow }),
            Check("mini player track (outline) on surfaceContainerHighest", { it.outline }, { it.surfaceContainerHighest }),
            Check("banner track on secondaryContainer", { bannerTrackOf(it) }, { it.secondaryContainer }),
            Check("active track (primary) on surface", { it.primary }, { it.surface }),
            Check("active track (primary) on surfaceContainerHighest", { it.primary }, { it.surfaceContainerHighest }),
            Check("selected swatch ring (onSurface) on surface", { it.onSurface }, { it.surface }),
        )

    private fun quietInk(scheme: ColorScheme, ground: Color): Color =
        scheme.onSurface.copy(alpha = QUIET_ALPHA).compositeOver(ground)

    private fun lyricGrounds(scheme: ColorScheme, washed: Boolean): List<Ground> =
        listOf(
            Ground("surface", scheme.surface),
            Ground("surfaceContainerLow", scheme.surfaceContainerLow),
        ) +
            if (washed)
                listOf(
                    Ground("primaryContainer at full strength", scheme.primaryContainer),
                    Ground(
                        "primaryContainer at the default strength",
                        scheme.primaryContainer.copy(alpha = 0.7f).compositeOver(scheme.surfaceContainerLow),
                    ),
                )
            else emptyList()

    private class Ground(val name: String, val ground: Color)

    @Test
    fun text_pairs_reach_4_5_to_1_for_every_accent_in_light_dark_and_pitch_black() {
        val failed = failures(0.0, textPairs, 4.5)
        assertTrue(failed.joinToString("\n"), failed.isEmpty())
    }

    @Test
    fun the_player_ink_reaches_4_5_to_1_over_the_cover_wash_at_full_strength() {
        val wash = listOf(Check("onSurface on primaryContainer wash", { it.onSurface }, { it.primaryContainer }))
        val failed = failures(0.0, wash, 4.5)
        assertTrue(failed.joinToString("\n"), failed.isEmpty())
    }

    @Test
    fun text_pairs_reach_4_5_to_1_under_high_contrast() {
        val failed = failures(1.0, textPairs, 4.5)
        assertTrue(failed.joinToString("\n"), failed.isEmpty())
    }

    @Test
    fun non_text_pairs_reach_3_to_1_for_every_accent_in_light_dark_and_pitch_black() {
        val failed = failures(0.0, nonTextPairs, 3.0)
        assertTrue(failed.joinToString("\n"), failed.isEmpty())
    }

    @Test
    fun non_text_pairs_reach_3_to_1_under_high_contrast() {
        val failed = failures(1.0, nonTextPairs, 3.0)
        assertTrue(failed.joinToString("\n"), failed.isEmpty())
    }

    @Test
    fun high_contrast_body_text_reaches_7_to_1() {
        val body =
            listOf(
                Check("onSurface on surface", { it: ColorScheme -> it.onSurface }, { it: ColorScheme -> it.surface }),
                Check("onSurfaceVariant on surface", { it.onSurfaceVariant }, { it.surface }),
            )
        val failed = failures(1.0, body, 7.0)
        assertTrue(failed.joinToString("\n"), failed.isEmpty())
    }

    @Test
    fun a_quiet_lyric_line_reaches_3_to_1_as_large_bold_text_on_every_ground() {
        val failed =
            listOf(0.0, 1.0).flatMap { contrast ->
                variants(contrast).flatMap { variant ->
                    lyricGrounds(variant.scheme, washed = contrast == 0.0).mapNotNull { ground ->
                        val found = ratio(quietInk(variant.scheme, ground.ground), ground.ground)
                        if (found < 3.0) "${variant.name}: quiet ink on ${ground.name} ${"%.2f".format(found)}"
                        else null
                    }
                }
            }
        assertTrue(failed.joinToString("\n"), failed.isEmpty())
    }

    @Test
    fun a_quiet_lyric_line_stays_quieter_than_the_active_one() {
        variants(0.0).forEach { variant ->
            val ground = variant.scheme.surface
            val active = ratio(variant.scheme.onSurface, ground)
            val quiet = ratio(quietInk(variant.scheme, ground), ground)
            assertTrue(variant.name, quiet < active)
        }
    }

    @Test
    fun high_contrast_lifts_the_body_text_of_every_variant() {
        variants(0.0).zip(variants(1.0)).forEach { (standard, high) ->
            val before = ratio(standard.scheme.onSurfaceVariant, standard.scheme.surface)
            val after = ratio(high.scheme.onSurfaceVariant, high.scheme.surface)
            assertTrue("${standard.name} $before -> $after", after >= before)
        }
    }

    @Test
    fun a_title_keeps_one_line_until_the_text_is_large() {
        assertEquals(1, titleLinesAt(1f))
        assertEquals(1, titleLinesAt(1.3f))
        assertEquals(2, titleLinesAt(1.5f))
        assertEquals(3, titleLinesAt(2f))
    }
}
