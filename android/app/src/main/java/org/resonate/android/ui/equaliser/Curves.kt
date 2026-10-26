package org.resonate.android.ui.equaliser

import java.util.Locale
import kotlin.math.abs
import kotlin.math.roundToInt
import org.resonate.android.nativecore.EqualiserBand
import org.resonate.android.nativecore.EqualiserCurve
import org.resonate.android.nativecore.EqualiserFilter
import org.resonate.android.nativecore.EqualiserLimits
import org.resonate.android.nativecore.EqualiserPreset
import org.resonate.android.nativecore.PlaybackPreferences

data class EqualiserBounds(
    val bandsAtMost: Int,
    val lowestHz: Float,
    val highestHz: Float,
    val gainAtMostDb: Float,
    val preampAtMostDb: Float,
    val qAtLeast: Float,
    val qAtMost: Float,
) {
    val hertz: ClosedFloatingPointRange<Float>
        get() = lowestHz..highestHz

    val gain: ClosedFloatingPointRange<Float>
        get() = -gainAtMostDb..gainAtMostDb

    val preamp: ClosedFloatingPointRange<Float>
        get() = -preampAtMostDb..preampAtMostDb

    val q: ClosedFloatingPointRange<Float>
        get() = qAtLeast..qAtMost

    companion object {
        val unread = EqualiserBounds(32, 20f, 20_000f, 24f, 24f, 0.1f, 40f)

        fun of(limits: EqualiserLimits): EqualiserBounds =
            EqualiserBounds(
                bandsAtMost = limits.bandsAtMost.toInt().takeIf { it > 0 } ?: unread.bandsAtMost,
                lowestHz = limits.lowestHz.takeIf { it > 0f } ?: unread.lowestHz,
                highestHz = limits.highestHz.takeIf { it > 0f } ?: unread.highestHz,
                gainAtMostDb = limits.gainAtMostDb.takeIf { it > 0f } ?: unread.gainAtMostDb,
                preampAtMostDb = limits.preampAtMostDb.takeIf { it > 0f } ?: unread.preampAtMostDb,
                qAtLeast = limits.qAtLeast.takeIf { it > 0f } ?: unread.qAtLeast,
                qAtMost = limits.qAtMost.takeIf { it > 0f } ?: unread.qAtMost,
            )
    }
}

val octaveCentres = listOf(31.25f, 62.5f, 125f, 250f, 500f, 1000f, 2000f, 4000f, 8000f, 16_000f)

const val OCTAVE_Q = 1.41421356f

const val FLAT_NAME = "Flat"

const val CUSTOM_NAME = "Custom"

val flatCurve = EqualiserCurve("", 0f, emptyList())

val EqualiserFilter.usesGain: Boolean
    get() =
        this == EqualiserFilter.PEAKING ||
            this == EqualiserFilter.LOW_SHELF ||
            this == EqualiserFilter.HIGH_SHELF

val EqualiserFilter.label: String
    get() =
        when (this) {
            EqualiserFilter.PEAKING -> "Peak"
            EqualiserFilter.LOW_SHELF -> "Low shelf"
            EqualiserFilter.HIGH_SHELF -> "High shelf"
            EqualiserFilter.LOW_PASS -> "Low pass"
            EqualiserFilter.HIGH_PASS -> "High pass"
            EqualiserFilter.NOTCH -> "Notch"
            EqualiserFilter.BAND_PASS -> "Band pass"
            EqualiserFilter.ALL_PASS -> "All pass"
        }

fun tidyHertz(hertz: Float): Float =
    when {
        hertz < 100f -> (hertz * 10).roundToInt() / 10f
        hertz < 1000f -> hertz.roundToInt().toFloat()
        else -> (hertz / 10).roundToInt() * 10f
    }

fun tidyDb(decibels: Float): Float = (decibels * 10).roundToInt() / 10f

fun tidyQ(q: Float): Float = (q * 100).roundToInt() / 100f

fun quarterDb(decibels: Float): Float = (decibels * 4).roundToInt() / 4f

fun hertzLabel(hertz: Float): String =
    when {
        hertz < 100f -> String.format(Locale.ROOT, "%.1f Hz", hertz)
        hertz < 1000f -> String.format(Locale.ROOT, "%.0f Hz", hertz)
        hertz < 10_000f -> String.format(Locale.ROOT, "%.2f kHz", hertz / 1000)
        else -> String.format(Locale.ROOT, "%.1f kHz", hertz / 1000)
    }

fun shortHertz(hertz: Float): String =
    if (hertz < 1000f) hertz.roundToInt().toString()
    else {
        val kilohertz = (hertz / 100).roundToInt() / 10f
        if (kilohertz % 1f == 0f) "${kilohertz.toInt()}k"
        else String.format(Locale.ROOT, "%.1fk", kilohertz)
    }

fun signedDb(decibels: Float): String {
    val shown = tidyDb(decibels).let { if (it == 0f) 0f else it }
    return (if (shown > 0f) "+" else "") + String.format(Locale.ROOT, "%.1f dB", shown)
}

fun qLabel(q: Float): String = String.format(Locale.ROOT, "Q %.2f", q)

fun audibleRange(hertz: Float): String =
    when {
        hertz < 60f -> "Sub-bass"
        hertz < 180f -> "Bass"
        hertz < 400f -> "Warmth"
        hertz < 900f -> "Low mids"
        hertz < 1800f -> "Mids"
        hertz < 3500f -> "Vocals"
        hertz < 6000f -> "Presence"
        hertz < 12_000f -> "Treble"
        else -> "Air"
    }

fun bandName(band: EqualiserBand): String = "${hertzLabel(band.frequencyHz)} ${audibleRange(band.frequencyHz)}"

fun level(band: EqualiserBand, preampDb: Float): Float =
    if (band.filter.usesGain) band.gainDb + preampDb else preampDb

fun isFlat(curve: EqualiserCurve): Boolean =
    curve.preampDb == 0f && curve.bands.all { it.filter.usesGain && it.gainDb == 0f }

fun shownName(curve: EqualiserCurve): String =
    when {
        isFlat(curve) -> FLAT_NAME
        curve.name.isNotEmpty() -> curve.name
        else -> CUSTOM_NAME
    }

fun shownPreset(presets: List<EqualiserPreset>, curve: EqualiserCurve): EqualiserPreset? {
    val name = shownName(curve)
    return presets.firstOrNull { it.name == name }
}

fun octaveCurve(
    frequencies: List<Float>,
    preampDb: Float = 0f,
    gains: List<Float>? = null,
    name: String = "",
): EqualiserCurve {
    val centres = frequencies.takeIf { it.size == octaveCentres.size } ?: octaveCentres
    return EqualiserCurve(
        name,
        preampDb,
        centres.mapIndexed { index, hertz ->
            EqualiserBand(EqualiserFilter.PEAKING, hertz, gains?.getOrNull(index) ?: 0f, OCTAVE_Q)
        },
    )
}

fun editable(curve: EqualiserCurve, frequencies: List<Float>): EqualiserCurve =
    if (curve.bands.isEmpty()) octaveCurve(frequencies, curve.preampDb) else curve

fun EqualiserCurve.edited(bands: List<EqualiserBand> = this.bands, preampDb: Float = this.preampDb) =
    EqualiserCurve("", preampDb, bands)

fun EqualiserCurve.withBand(index: Int, band: EqualiserBand): EqualiserCurve =
    if (index !in bands.indices) this
    else edited(bands = bands.toMutableList().also { it[index] = band })

fun EqualiserCurve.withGain(index: Int, gainDb: Float, bounds: EqualiserBounds): EqualiserCurve =
    bands.getOrNull(index)?.let { withBand(index, it.copy(gainDb = tidyDb(gainDb.coerceIn(bounds.gain)))) }
        ?: this

fun EqualiserCurve.withoutBand(index: Int): EqualiserCurve =
    if (index !in bands.indices) this
    else edited(bands = bands.toMutableList().also { it.removeAt(index) })

fun EqualiserCurve.withAddedBand(hertz: Float, gainDb: Float, bounds: EqualiserBounds): EqualiserCurve? =
    if (bands.size >= bounds.bandsAtMost) null
    else
        edited(
            bands =
                bands +
                    EqualiserBand(
                        EqualiserFilter.PEAKING,
                        tidyHertz(hertz.coerceIn(bounds.hertz)),
                        tidyDb(gainDb.coerceIn(bounds.gain)),
                        1f,
                    )
        )

fun EqualiserCurve.withPreamp(decibels: Float, bounds: EqualiserBounds): EqualiserCurve =
    edited(preampDb = tidyDb(decibels.coerceIn(bounds.preamp)))

fun EqualiserCurve.withFittedPreamp(decibels: Float, bounds: EqualiserBounds): EqualiserCurve =
    edited(preampDb = decibels.coerceIn(bounds.preamp))

fun PlaybackPreferences.withPreset(preset: EqualiserPreset): PlaybackPreferences =
    copy(equaliserEnabled = true, equaliser = preset.curve)

fun PlaybackPreferences.withCurve(curve: EqualiserCurve): PlaybackPreferences =
    copy(equaliserEnabled = true, equaliser = curve)

fun PlaybackPreferences.reset(): PlaybackPreferences = copy(equaliser = flatCurve)

fun sliderCap(band: EqualiserBand, bounds: EqualiserBounds): Float =
    minOf(bounds.gainAtMostDb, maxOf(SLIDER_REACH_DB, abs(band.gainDb)))

const val SLIDER_REACH_DB = 12f
