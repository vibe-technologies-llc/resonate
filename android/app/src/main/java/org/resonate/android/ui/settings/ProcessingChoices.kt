package org.resonate.android.ui.settings

import kotlin.math.roundToInt
import org.resonate.android.nativecore.CrossfeedLevel
import org.resonate.android.nativecore.GainMode
import org.resonate.android.nativecore.LossyRestoration
import org.resonate.android.nativecore.ResamplingPhase
import org.resonate.android.nativecore.ResamplingQuality

const val BUFFER_LEAST_MS = 50
const val BUFFER_MOST_MS = 2000
const val BUFFER_STEP_MS = 10

fun bufferStep(value: Float): Int =
    ((value / BUFFER_STEP_MS).roundToInt() * BUFFER_STEP_MS).coerceIn(
        BUFFER_LEAST_MS,
        BUFFER_MOST_MS,
    )

val gainChoices =
    listOf(
        Choice(GainMode.OFF, "Off", "Play every song at its own level"),
        Choice(GainMode.TRACK, "Track", "Even out loudness from song to song"),
        Choice(GainMode.ALBUM, "Album", "Keep an album's own dynamics between its songs"),
    )

val qualityChoices =
    listOf(
        Choice(ResamplingQuality.FAST, "Fast", "Least battery, a softer filter"),
        Choice(ResamplingQuality.BALANCED, "Balanced", "A good filter at modest cost"),
        Choice(ResamplingQuality.HIGH, "High", "A steep filter for critical listening"),
        Choice(ResamplingQuality.VERY_HIGH, "Very high", "The steepest filter, the most battery"),
    )

val phaseChoices =
    listOf(
        Choice(ResamplingPhase.LINEAR, "Linear", "No phase shift, with ringing before transients"),
        Choice(ResamplingPhase.INTERMEDIATE, "Intermediate", "A balance of the two"),
        Choice(
            ResamplingPhase.MINIMUM,
            "Minimum",
            "No ringing before transients, a little phase shift",
        ),
    )

val crossfeedChoices =
    listOf(
        Choice(
            CrossfeedLevel.OFF,
            "Off",
            "Each channel stays in its own ear, exactly as the record was mixed",
        ),
        Choice(
            CrossfeedLevel.LIGHT,
            "Light",
            "A faint, low copy of each side reaches the other ear (650 Hz, 9.5 dB down), enough to take the edge off a hard-panned record",
        ),
        Choice(
            CrossfeedLevel.MODERATE,
            "Moderate",
            "The classic setting (700 Hz, 6 dB down): the image closes up toward the front without losing its width",
        ),
        Choice(
            CrossfeedLevel.STRONG,
            "Strong",
            "The most of the other side reaches each ear (700 Hz, 4.5 dB down), for old stereo records with the voices on one side and the band on the other",
        ),
    )

val restorationChoices =
    listOf(
        Choice(LossyRestoration.OFF, "Off", "Play lossy files as they were decoded"),
        Choice(
            LossyRestoration.REPAIR,
            "Repair",
            "Lift the treble droop under the encoder's cut and fill its holes",
        ),
        Choice(LossyRestoration.EXTEND, "Extend", "Repair, and also rebuild the treble it cut"),
    )

fun <T> labelOf(choices: List<Choice<T>>, value: T): String =
    choices.firstOrNull { it.value == value }?.label ?: ""
