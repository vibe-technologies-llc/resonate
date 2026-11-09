package org.resonate.android.ui.settings

import androidx.compose.runtime.Composable
import kotlin.math.roundToInt
import org.resonate.android.R
import org.resonate.android.Words
import org.resonate.android.nativecore.CrossfeedLevel
import org.resonate.android.nativecore.GainMode
import org.resonate.android.nativecore.LossyRestoration
import org.resonate.android.nativecore.ResamplingPhase
import org.resonate.android.nativecore.ResamplingQuality
import org.resonate.android.words

const val BUFFER_LEAST_MS = 50
const val BUFFER_MOST_MS = 2000
const val BUFFER_STEP_MS = 10

fun bufferStep(value: Float): Int =
    ((value / BUFFER_STEP_MS).roundToInt() * BUFFER_STEP_MS).coerceIn(
        BUFFER_LEAST_MS,
        BUFFER_MOST_MS,
    )

fun gainChoices(words: Words): List<Choice<GainMode>> =
    listOf(
        Choice(
            GainMode.OFF,
            words.string(R.string.processing_off),
            words.string(R.string.processing_gain_off_detail),
        ),
        Choice(
            GainMode.TRACK,
            words.string(R.string.processing_gain_track),
            words.string(R.string.processing_gain_track_detail),
        ),
        Choice(
            GainMode.ALBUM,
            words.string(R.string.processing_gain_album),
            words.string(R.string.processing_gain_album_detail),
        ),
    )

val gainChoices: List<Choice<GainMode>>
    @Composable get() = gainChoices(words())

fun qualityChoices(words: Words): List<Choice<ResamplingQuality>> =
    listOf(
        Choice(
            ResamplingQuality.LOW_LATENCY,
            words.string(R.string.processing_quality_low_latency),
            words.string(R.string.processing_quality_low_latency_detail),
        ),
        Choice(
            ResamplingQuality.BALANCED,
            words.string(R.string.processing_quality_balanced),
            words.string(R.string.processing_quality_balanced_detail),
        ),
        Choice(
            ResamplingQuality.HIGH,
            words.string(R.string.processing_quality_high),
            words.string(R.string.processing_quality_high_detail),
        ),
        Choice(
            ResamplingQuality.VERY_HIGH,
            words.string(R.string.processing_quality_very_high),
            words.string(R.string.processing_quality_very_high_detail),
        ),
    )

val qualityChoices: List<Choice<ResamplingQuality>>
    @Composable get() = qualityChoices(words())

fun phaseChoices(words: Words): List<Choice<ResamplingPhase>> =
    listOf(
        Choice(
            ResamplingPhase.LINEAR,
            words.string(R.string.processing_phase_linear),
            words.string(R.string.processing_phase_linear_detail),
        ),
        Choice(
            ResamplingPhase.INTERMEDIATE,
            words.string(R.string.processing_phase_intermediate),
            words.string(R.string.processing_phase_intermediate_detail),
        ),
        Choice(
            ResamplingPhase.MINIMUM,
            words.string(R.string.processing_phase_minimum),
            words.string(R.string.processing_phase_minimum_detail),
        ),
    )

val phaseChoices: List<Choice<ResamplingPhase>>
    @Composable get() = phaseChoices(words())

fun crossfeedChoices(words: Words): List<Choice<CrossfeedLevel>> =
    listOf(
        Choice(
            CrossfeedLevel.OFF,
            words.string(R.string.processing_off),
            words.string(R.string.processing_crossfeed_off_detail),
        ),
        Choice(
            CrossfeedLevel.LIGHT,
            words.string(R.string.processing_crossfeed_light),
            words.string(R.string.processing_crossfeed_light_detail),
        ),
        Choice(
            CrossfeedLevel.MODERATE,
            words.string(R.string.processing_crossfeed_moderate),
            words.string(R.string.processing_crossfeed_moderate_detail),
        ),
        Choice(
            CrossfeedLevel.STRONG,
            words.string(R.string.processing_crossfeed_strong),
            words.string(R.string.processing_crossfeed_strong_detail),
        ),
    )

val crossfeedChoices: List<Choice<CrossfeedLevel>>
    @Composable get() = crossfeedChoices(words())

fun restorationChoices(words: Words): List<Choice<LossyRestoration>> =
    listOf(
        Choice(
            LossyRestoration.OFF,
            words.string(R.string.processing_off),
            words.string(R.string.processing_restoration_off_detail),
        ),
        Choice(
            LossyRestoration.REPAIR,
            words.string(R.string.processing_restoration_repair),
            words.string(R.string.processing_restoration_repair_detail),
        ),
        Choice(
            LossyRestoration.EXTEND,
            words.string(R.string.processing_restoration_extend),
            words.string(R.string.processing_restoration_extend_detail),
        ),
    )

val restorationChoices: List<Choice<LossyRestoration>>
    @Composable get() = restorationChoices(words())

fun <T> labelOf(choices: List<Choice<T>>, value: T): String =
    choices.firstOrNull { it.value == value }?.label ?: ""
