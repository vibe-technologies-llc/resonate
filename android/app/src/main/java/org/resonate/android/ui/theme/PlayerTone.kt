package org.resonate.android.ui.theme

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.toArgb
import com.materialkolor.hct.Hct
import kotlin.math.max
import kotlin.math.min

object PlayerTone {
    const val NEUTRAL_INTENSITY = 0.5f

    private const val DARK_DIMMING = 0.5
    private const val LIGHT_DIMMING = 0.25
    private const val CHROMA_DULLING = 0.4
    private const val CHROMA_ENRICHING = 0.5
    private const val TONE_LIFT = 0.12
    private const val BRIGHTEST_TONE = 95.0

    fun top(color: Color, intensity: Float, dark: Boolean): Color {
        val lean = (intensity.coerceIn(0f, 1f) - NEUTRAL_INTENSITY) * 2
        if (lean == 0f) return color

        val darker = max(-lean, 0f).toDouble()
        val livelier = max(lean, 0f).toDouble()
        val dimming = 1 - (if (dark) DARK_DIMMING else LIGHT_DIMMING) * darker

        val held = Hct.fromInt(color.toArgb())
        val tone = min(held.tone * dimming * (1 + TONE_LIFT * livelier), max(BRIGHTEST_TONE, held.tone))
        val chroma = held.chroma * (1 - CHROMA_DULLING * darker + CHROMA_ENRICHING * livelier)

        return Color(Hct.from(held.hue, chroma, tone).toInt())
    }
}
