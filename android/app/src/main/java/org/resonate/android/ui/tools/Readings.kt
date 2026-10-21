package org.resonate.android.ui.tools

import java.util.Locale
import kotlin.math.PI
import kotlin.math.abs
import kotlin.math.cos
import kotlin.math.log10
import kotlin.math.max
import kotlin.math.pow
import kotlin.math.roundToInt
import kotlin.math.sin
import kotlin.math.sqrt
import org.resonate.android.nativecore.AnalysisReport
import org.resonate.android.nativecore.Inspector
import org.resonate.android.nativecore.Track
import org.resonate.android.ui.clock

enum class Tone {
    GOOD,
    WARNING,
    BAD,
    NEUTRAL,
}

data class Verdict(val title: String, val detail: String, val tone: Tone)

fun verdictOf(name: String): Verdict =
    when (name) {
        "genuine" -> Verdict("Consistent with lossless", "Nothing in the audio suggests a lossy source.", Tone.GOOD)
        "suspect" -> Verdict("Suspect", "It may be a high-bitrate lossy encode.", Tone.WARNING)
        "fake" -> Verdict("Not truly lossless", "It is not the lossless master it claims to be.", Tone.BAD)
        "lossy" -> Verdict("Lossy by its codec", "The format itself discards audio, so there is nothing to judge.", Tone.NEUTRAL)
        else -> Verdict("Not judged", "There was not enough to go on.", Tone.NEUTRAL)
    }

fun decibelsOf(level: Float): Float? = if (level > 0f) 20 * log10(level) else null

fun signed(value: Float, digits: Int, unit: String): String {
    val shown = String.format(Locale.ROOT, "%.${digits}f", value)
    val zero = shown.trimStart('-').all { it == '0' || it == '.' }
    return (if (value > 0f && !zero) "+" else "") + (if (zero) shown.trimStart('-') else shown) + " $unit"
}

data class Metric(val label: String, val value: String, val detail: String)

fun metricsOf(report: AnalysisReport): List<Metric> =
    listOfNotNull(
        report.loudnessLufs?.let {
            Metric("Loudness", String.format(Locale.ROOT, "%.1f LUFS", it), "Integrated, as streaming services measure")
        },
        report.dynamicRange?.let { Metric("Dynamic range", "DR$it", "Higher is less compressed") },
        decibelsOf(report.peak)?.let { Metric("Peak", signed(it, 2, "dBFS"), "Loudest sample") },
        decibelsOf(report.truePeak)?.let { Metric("True peak", signed(it, 2, "dBTP"), "Between samples, after reconstruction") },
        decibelsOf(report.rms)?.let { Metric("RMS", signed(it, 2, "dBFS"), "Average level") },
        Metric(
            "Clipped samples",
            String.format(Locale.ROOT, "%,d", report.clipped.toLong()),
            if (report.clipped == 0uL) "None at full scale" else "At full scale",
        ),
        report.bitsInUse?.let { Metric("Bits in use", "$it bit", "Depth the samples actually reach") },
    )

const val SPECTRUM_FLOOR_DB = -120f

fun spectrumHeights(levels: List<Float>, floorDb: Float = SPECTRUM_FLOOR_DB): List<Float> =
    levels.map { level -> if (level.isFinite()) ((level - floorDb) / -floorDb).coerceIn(0f, 1f) else 0f }

fun spectrumMarks(count: Int, stepHz: Float): List<Pair<Float, String>> {
    val top = count * stepHz
    if (count < 2 || stepHz <= 0f) return emptyList()
    val everyHz = if (top > WIDE_SPECTRUM_HZ) 10_000f else 5_000f
    return generateSequence(everyHz) { it + everyHz }
        .takeWhile { it < top }
        .map { hertz -> hertz / top to "${(hertz / 1000).roundToInt()}k" }
        .toList()
}

private const val WIDE_SPECTRUM_HZ = 30_000f

fun waveformHeights(values: List<Float>): List<Float> {
    val loudest = values.filter { it.isFinite() }.maxOfOrNull { abs(it) } ?: 0f
    return if (loudest <= 0f) values.map { 0f } else values.map { (abs(it) / loudest).coerceIn(0f, 1f) }
}

data class Detail(val label: String, val value: String, val copyable: Boolean = false)

private fun Detail?.kept(): Detail? = this?.takeIf { it.value.isNotBlank() }

fun technicalDetails(track: Track, inspector: Inspector?): List<Detail> =
    listOfNotNull(
        inspector?.container?.let { Detail("Container", it) }.kept(),
        Detail("Codec", track.codec).kept(),
        inspector?.sampleFormat?.let { Detail("Sample format", it) }.kept(),
        track.rate.takeIf { it > 0u }?.let { Detail("Sample rate", String.format(Locale.ROOT, "%,d Hz", it.toLong())) },
        track.bits?.let { Detail("Bit depth", "$it bit") },
        track.channels.takeIf { it > 0u }?.let { Detail("Channels", channelsOf(it)) },
        Detail("Duration", clock(track.durationMs)),
        inspector?.let { Detail("Seekable", if (it.seekable) "Yes" else "No") },
        inspector?.encoder?.let { Detail("Encoder", it) }.kept(),
    )

fun taggedDetails(track: Track, inspector: Inspector?): List<Detail> =
    listOfNotNull(
        Detail("Title", track.title, copyable = true).kept(),
        Detail("Artist", track.artist, copyable = true).kept(),
        Detail("Album", track.album, copyable = true).kept(),
        Detail("Album artist", track.albumArtist).kept(),
        Detail("Genre", track.genre).kept(),
        Detail("Date", track.date).kept(),
        track.trackNumber?.let { Detail("Track", "$it") },
        track.discNumber?.let { Detail("Disc", "$it") },
        inspector?.isrc?.let { Detail("ISRC", it, copyable = true) }.kept(),
        track.recording?.let { Detail("MusicBrainz recording", it, copyable = true) }.kept(),
        track.release?.let { Detail("MusicBrainz release", it, copyable = true) }.kept(),
        track.artistMbid?.let { Detail("MusicBrainz artist", it, copyable = true) }.kept(),
        inspector?.copyright?.let { Detail("Copyright", it) }.kept(),
        inspector?.comment?.let { Detail("Comment", it, copyable = true) }.kept(),
        Detail("File", track.name, copyable = true).kept(),
    )

fun channelsOf(count: UInt): String =
    when (count) {
        1u -> "Mono"
        2u -> "Stereo"
        else -> "$count channels"
    }

fun scoreOf(score: UInt): String = "${score.coerceAtMost(100u)}% match"

class Bands(private val count: Int, private val window: Int) {
    private val bins: List<IntRange> = buildList {
        val highest = window / 2 - 1
        var next = 1
        repeat(count) { band ->
            val edge = highest.toDouble().pow((band + 1).toDouble() / count).roundToInt()
            val end = max(next, edge).coerceAtMost(highest)
            add(next..end)
            next = (end + 1).coerceAtMost(highest)
        }
    }

    fun levels(samples: List<Float>): List<Float> {
        if (samples.size < window) return List(count) { 0f }
        val shaped = samples.take(window).mapIndexed { index, sample -> sample * hann(index) }
        return bins.map { range ->
            val loudest = range.maxOf { bin -> magnitude(shaped, bin) }
            val decibels = 20 * log10(loudest.coerceAtLeast(1e-6f))
            ((decibels - FLOOR) / -FLOOR).coerceIn(0f, 1f)
        }
    }

    private fun hann(index: Int): Float = (0.5 - 0.5 * cos(2 * PI * index / (window - 1))).toFloat()

    private fun magnitude(samples: List<Float>, bin: Int): Float {
        var real = 0.0
        var imaginary = 0.0
        samples.forEachIndexed { index, sample ->
            val angle = 2 * PI * bin * index / window
            real += sample * cos(angle)
            imaginary -= sample * sin(angle)
        }
        return (sqrt(real * real + imaginary * imaginary) * 4 / window).toFloat()
    }

    private companion object {
        const val FLOOR = -72f
    }
}

fun eased(previous: List<Float>, next: List<Float>, rise: Float = 0.6f, fall: Float = 0.18f): List<Float> =
    if (previous.size != next.size) next
    else previous.zip(next) { was, now -> was + (now - was) * if (now > was) rise else fall }

fun loudnessOf(samples: List<Float>): Float =
    if (samples.isEmpty()) 0f else sqrt(samples.sumOf { (it * it).toDouble() } / samples.size).toFloat()
