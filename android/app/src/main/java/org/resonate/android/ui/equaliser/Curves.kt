package org.resonate.android.ui.equaliser

import androidx.annotation.StringRes
import java.util.Locale
import kotlin.math.abs
import kotlin.math.exp
import kotlin.math.ln
import kotlin.math.pow
import kotlin.math.roundToInt
import org.resonate.android.R
import org.resonate.android.Words
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

val flatCurve = EqualiserCurve("", 0f, emptyList())

val EqualiserFilter.usesGain: Boolean
    get() =
        this == EqualiserFilter.PEAKING ||
            this == EqualiserFilter.LOW_SHELF ||
            this == EqualiserFilter.HIGH_SHELF

@get:StringRes
val EqualiserFilter.label: Int
    get() =
        when (this) {
            EqualiserFilter.PEAKING -> R.string.equaliser_filter_peak
            EqualiserFilter.LOW_SHELF -> R.string.equaliser_filter_low_shelf
            EqualiserFilter.HIGH_SHELF -> R.string.equaliser_filter_high_shelf
            EqualiserFilter.LOW_PASS -> R.string.equaliser_filter_low_pass
            EqualiserFilter.HIGH_PASS -> R.string.equaliser_filter_high_pass
            EqualiserFilter.NOTCH -> R.string.equaliser_filter_notch
            EqualiserFilter.BAND_PASS -> R.string.equaliser_filter_band_pass
            EqualiserFilter.ALL_PASS -> R.string.equaliser_filter_all_pass
        }

fun tidyHertz(hertz: Float): Float =
    when {
        hertz < 100f -> (hertz * 10).roundToInt() / 10f
        hertz < 1000f -> hertz.roundToInt().toFloat()
        else -> (hertz / 10).roundToInt() * 10f
    }

fun tidyDb(decibels: Float): Float = (decibels * 10).roundToInt() / 10f

fun tidyQ(q: Float): Float = (q * 100).roundToInt() / 100f

fun hertzLabel(words: Words, hertz: Float): String =
    when {
        hertz < 100f ->
            words.string(R.string.equaliser_hertz, String.format(Locale.ROOT, "%.1f", hertz))
        hertz < 1000f ->
            words.string(R.string.equaliser_hertz, String.format(Locale.ROOT, "%.0f", hertz))
        hertz < 10_000f ->
            words.string(R.string.unit_kilohertz, String.format(Locale.ROOT, "%.2f", hertz / 1000))
        else ->
            words.string(R.string.unit_kilohertz, String.format(Locale.ROOT, "%.1f", hertz / 1000))
    }

fun signedDb(words: Words, decibels: Float): String {
    val shown = tidyDb(decibels).let { if (it == 0f) 0f else it }
    val figure = (if (shown > 0f) "+" else "") + String.format(Locale.ROOT, "%.1f", shown)
    return words.string(R.string.equaliser_decibels, figure)
}

fun qLabel(words: Words, q: Float): String =
    words.string(R.string.equaliser_q, String.format(Locale.ROOT, "%.2f", q))

@StringRes
fun audibleRange(hertz: Float): Int =
    when {
        hertz < 60f -> R.string.equaliser_range_sub_bass
        hertz < 180f -> R.string.equaliser_range_bass
        hertz < 400f -> R.string.equaliser_range_warmth
        hertz < 900f -> R.string.equaliser_range_low_mids
        hertz < 1800f -> R.string.equaliser_range_mids
        hertz < 3500f -> R.string.equaliser_range_vocals
        hertz < 6000f -> R.string.equaliser_range_presence
        hertz < 12_000f -> R.string.equaliser_range_treble
        else -> R.string.equaliser_range_air
    }

fun bandName(words: Words, band: EqualiserBand): String =
    "${hertzLabel(words, band.frequencyHz)} ${words.string(audibleRange(band.frequencyHz))}"

fun level(band: EqualiserBand, preampDb: Float): Float =
    if (band.filter.usesGain) band.gainDb + preampDb else preampDb

fun isFlat(curve: EqualiserCurve): Boolean =
    curve.preampDb == 0f && curve.bands.all { it.filter.usesGain && it.gainDb == 0f }

fun shownName(words: Words, curve: EqualiserCurve): String =
    when {
        isFlat(curve) -> words.string(R.string.equaliser_flat)
        curve.name.isNotEmpty() -> curve.name
        else -> words.string(R.string.equaliser_custom)
    }

fun shownPreset(presets: List<EqualiserPreset>, curve: EqualiserCurve): EqualiserPreset? =
    if (isFlat(curve)) presets.firstOrNull { isFlat(it.curve) }
    else presets.firstOrNull { curve.name.isNotEmpty() && it.name == curve.name }

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
                        ADDED_BAND_Q,
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

const val ADDED_BAND_HERTZ = 1000f

const val ADDED_BAND_Q = 1f

const val GAIN_STEP_DB = 0.5f

const val HERTZ_STEP_OCTAVES = 1f / 3f

private const val GAPS_ALIKE_WITHIN = 1e-6

fun freeHertz(curve: EqualiserCurve, bounds: EqualiserBounds): Float {
    val taken = curve.bands.map { ln(it.frequencyHz.coerceIn(bounds.hertz).toDouble()) }.sorted()
    if (taken.isEmpty()) return ADDED_BAND_HERTZ.coerceIn(bounds.hertz)
    val edges = listOf(ln(bounds.lowestHz.toDouble())) + taken + ln(bounds.highestHz.toDouble())
    val gaps = edges.zipWithNext { low, high -> (high - low) to (low + high) / 2 }
    val widest = gaps.maxOf { it.first }
    val centre = ln(ADDED_BAND_HERTZ.toDouble())
    val middles = gaps.filter { it.first >= widest - GAPS_ALIKE_WITHIN }.map { it.second }
    val nearest = middles.minOf { abs(it - centre) }
    val middle = middles.first { abs(it - centre) <= nearest + GAPS_ALIKE_WITHIN }
    return tidyHertz(exp(middle).toFloat().coerceIn(bounds.hertz))
}

fun EqualiserCurve.withFreeBand(bounds: EqualiserBounds): EqualiserCurve? =
    withAddedBand(freeHertz(this, bounds), 0f, bounds)

fun EqualiserCurve.withNudgedGain(index: Int, by: Float, bounds: EqualiserBounds): EqualiserCurve {
    val band = bands.getOrNull(index)?.takeIf { it.filter.usesGain } ?: return this
    return withGain(index, band.gainDb + by, bounds)
}

fun EqualiserCurve.withNudgedHertz(
    index: Int,
    octaves: Float,
    bounds: EqualiserBounds,
): EqualiserCurve {
    val band = bands.getOrNull(index) ?: return this
    val moved = (band.frequencyHz * 2f.pow(octaves)).coerceIn(bounds.hertz)
    return withBand(index, band.copy(frequencyHz = tidyHertz(moved)))
}

fun chosenBand(selected: Int?, count: Int): Int? =
    if (count <= 0) null else (selected ?: 0).coerceIn(0, count - 1)

fun chosenAfterRemoving(removed: Int, count: Int): Int? =
    if (count <= 1) null else removed.coerceAtMost(count - 2)
