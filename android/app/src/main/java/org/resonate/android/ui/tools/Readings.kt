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
import org.resonate.android.R
import org.resonate.android.Words
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

fun verdictOf(words: Words, name: String): Verdict {
    val (title, detail, tone) =
        when (name) {
            "genuine" ->
                Triple(
                    R.string.analysis_verdict_genuine,
                    R.string.analysis_verdict_genuine_detail,
                    Tone.GOOD,
                )
            "suspect" ->
                Triple(
                    R.string.analysis_verdict_suspect,
                    R.string.analysis_verdict_suspect_detail,
                    Tone.WARNING,
                )
            "fake" ->
                Triple(R.string.analysis_verdict_fake, R.string.analysis_verdict_fake_detail, Tone.BAD)
            "lossy" ->
                Triple(
                    R.string.analysis_verdict_lossy,
                    R.string.analysis_verdict_lossy_detail,
                    Tone.NEUTRAL,
                )
            else ->
                Triple(
                    R.string.analysis_verdict_unjudged,
                    R.string.analysis_verdict_unjudged_detail,
                    Tone.NEUTRAL,
                )
        }
    return Verdict(words.string(title), words.string(detail), tone)
}

fun decibelsOf(level: Float): Float? = if (level > 0f) 20 * log10(level) else null

fun signed(value: Float, digits: Int, unit: String): String {
    val shown = String.format(Locale.ROOT, "%.${digits}f", value)
    val zero = shown.trimStart('-').all { it == '0' || it == '.' }
    return (if (value > 0f && !zero) "+" else "") + (if (zero) shown.trimStart('-') else shown) + " $unit"
}

data class Metric(val label: String, val value: String, val detail: String)

fun metricsOf(words: Words, report: AnalysisReport): List<Metric> =
    listOfNotNull(
        report.loudnessLufs?.let {
            Metric(
                words.string(R.string.analysis_metric_loudness),
                words.string(R.string.analysis_metric_lufs, String.format(Locale.ROOT, "%.1f", it)),
                words.string(R.string.analysis_metric_loudness_detail),
            )
        },
        report.dynamicRange?.let {
            Metric(
                words.string(R.string.analysis_metric_dynamic_range),
                "DR$it",
                words.string(R.string.analysis_metric_dynamic_range_detail),
            )
        },
        decibelsOf(report.peak)?.let {
            Metric(
                words.string(R.string.analysis_metric_peak),
                signed(it, 2, "dBFS"),
                words.string(R.string.analysis_metric_peak_detail),
            )
        },
        decibelsOf(report.truePeak)?.let {
            Metric(
                words.string(R.string.analysis_metric_true_peak),
                signed(it, 2, "dBTP"),
                words.string(R.string.analysis_metric_true_peak_detail),
            )
        },
        decibelsOf(report.rms)?.let {
            Metric("RMS", signed(it, 2, "dBFS"), words.string(R.string.analysis_metric_rms_detail))
        },
        Metric(
            words.string(R.string.analysis_metric_clipped),
            String.format(Locale.ROOT, "%,d", report.clipped.toLong()),
            words.string(
                if (report.clipped == 0uL) R.string.analysis_metric_clipped_none
                else R.string.analysis_metric_clipped_some
            ),
        ),
        report.bitsInUse?.let {
            Metric(
                words.string(R.string.analysis_metric_bits_in_use),
                words.string(R.string.unit_bits, it.toInt()),
                words.string(R.string.analysis_metric_bits_in_use_detail),
            )
        },
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

fun technicalDetails(words: Words, track: Track, inspector: Inspector?): List<Detail> =
    listOfNotNull(
        inspector
            ?.container
            ?.let { Detail(words.string(R.string.song_detail_container), it) }
            .kept(),
        Detail(words.string(R.string.song_detail_codec), track.codec).kept(),
        inspector
            ?.sampleFormat
            ?.let { Detail(words.string(R.string.song_detail_sample_format), it) }
            .kept(),
        track.rate
            .takeIf { it > 0u }
            ?.let {
                Detail(
                    words.string(R.string.song_detail_sample_rate),
                    words.string(
                        R.string.song_detail_hertz,
                        String.format(Locale.ROOT, "%,d", it.toLong()),
                    ),
                )
            },
        track.bits?.let {
            Detail(
                words.string(R.string.song_detail_bit_depth),
                words.string(R.string.unit_bits, it.toInt()),
            )
        },
        track.channels
            .takeIf { it > 0u }
            ?.let { Detail(words.string(R.string.song_detail_channels), channelsOf(words, it)) },
        Detail(words.string(R.string.song_detail_duration), clock(track.durationMs)),
        inspector?.let {
            Detail(
                words.string(R.string.song_detail_seekable),
                words.string(
                    if (it.seekable) R.string.song_detail_yes else R.string.song_detail_no
                ),
            )
        },
        inspector?.encoder?.let { Detail(words.string(R.string.song_detail_encoder), it) }.kept(),
    )

fun taggedDetails(words: Words, track: Track, inspector: Inspector?): List<Detail> =
    listOfNotNull(
        Detail(words.string(R.string.song_detail_title), track.title, copyable = true).kept(),
        Detail(words.string(R.string.song_detail_artist), track.artist, copyable = true).kept(),
        Detail(words.string(R.string.song_detail_album), track.album, copyable = true).kept(),
        Detail(words.string(R.string.song_detail_album_artist), track.albumArtist).kept(),
        Detail(words.string(R.string.song_detail_genre), track.genre).kept(),
        Detail(words.string(R.string.song_detail_date), track.date).kept(),
        track.trackNumber?.let { Detail(words.string(R.string.song_detail_track), "$it") },
        track.discNumber?.let { Detail(words.string(R.string.song_detail_disc), "$it") },
        inspector?.isrc?.let { Detail("ISRC", it, copyable = true) }.kept(),
        track.recording
            ?.let { Detail(words.string(R.string.song_detail_recording), it, copyable = true) }
            .kept(),
        track.release
            ?.let { Detail(words.string(R.string.song_detail_release), it, copyable = true) }
            .kept(),
        track.artistMbid
            ?.let { Detail(words.string(R.string.song_detail_artist_mbid), it, copyable = true) }
            .kept(),
        inspector
            ?.copyright
            ?.let { Detail(words.string(R.string.song_detail_copyright), it) }
            .kept(),

        inspector
            ?.comment
            ?.let { Detail(words.string(R.string.song_detail_comment), it, copyable = true) }
            .kept(),
        Detail(words.string(R.string.song_detail_file), track.name, copyable = true).kept(),
    )

fun channelsOf(words: Words, count: UInt): String =
    when (count) {
        1u -> words.string(R.string.song_channels_mono)
        2u -> words.string(R.string.song_channels_stereo)
        else -> words.plural(R.plurals.song_channels_count, count.toInt(), count.toInt())
    }

fun scoreOf(words: Words, score: UInt): String =
    words.string(R.string.analysis_recognition_score, score.coerceAtMost(100u).toInt())


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
