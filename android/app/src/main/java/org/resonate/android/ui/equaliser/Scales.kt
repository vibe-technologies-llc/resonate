package org.resonate.android.ui.equaliser

import java.util.Locale
import kotlin.math.abs
import kotlin.math.ceil
import kotlin.math.hypot
import kotlin.math.ln
import kotlin.math.log10
import kotlin.math.max
import kotlin.math.pow
import org.resonate.android.nativecore.EqualiserCurve
import org.resonate.android.nativecore.RoomCorrection

object GraphScale {
    const val LOWEST_HZ = 20f
    const val HIGHEST_HZ = 20_000f
    const val NARROWEST_DB = 15f
    const val WIDEST_DB = 30f
    const val MARKED_EVERY_DB = 5f
    private const val DECADES = 3f

    val faintHertz =
        listOf(30f, 40f, 50f, 60f, 70f, 80f, 90f, 200f, 300f, 400f, 500f, 600f, 700f, 800f, 900f) +
            listOf(2000f, 3000f, 4000f, 5000f, 6000f, 7000f, 8000f, 9000f)

    val labelledHertz = listOf(100f to "100", 1000f to "1k", 10_000f to "10k")

    fun x(hertz: Float, width: Float): Float =
        log10(hertz.coerceIn(LOWEST_HZ, HIGHEST_HZ) / LOWEST_HZ) / DECADES * width

    fun hertzAt(x: Float, width: Float): Float =
        LOWEST_HZ * 1000f.pow((x / max(width, 1f)).coerceIn(0f, 1f))

    fun y(decibels: Float, height: Float, span: Float): Float =
        (span - decibels.coerceIn(-span, span)) / (2 * span) * height

    fun dbAt(y: Float, height: Float, span: Float): Float = span - y / max(height, 1f) * 2 * span

    fun responseX(index: Int, count: Int, width: Float): Float =
        if (count < 2) 0f else index.toFloat() / (count - 1) * width

    fun span(response: List<Float>, curve: EqualiserCurve): Float {
        val handles = curve.bands.map { level(it, curve.preampDb) }
        val loudest = (response + handles).filter { it.isFinite() }.maxOfOrNull { abs(it) } ?: 0f
        val rounded = ceil(loudest / MARKED_EVERY_DB) * MARKED_EVERY_DB
        return rounded.coerceIn(NARROWEST_DB, WIDEST_DB)
    }

    fun marks(span: Float): List<Float> =
        generateSequence(-span) { it + MARKED_EVERY_DB }.takeWhile { it <= span + 0.001f }.toList()
}

data class GraphPoint(val x: Float, val y: Float)

class GraphFrame(
    val width: Float,
    val height: Float,
    val span: Float,
    val inset: Float = 0f,
    val gutter: Float = 0f,
) {
    val left: Float
        get() = inset + gutter

    val plotWidth: Float
        get() = max(width - left - inset, 1f)

    val plotHeight: Float
        get() = max(height - 2 * inset, 1f)

    fun handle(curve: EqualiserCurve, index: Int): GraphPoint {
        val band = curve.bands[index]
        return GraphPoint(
            left + GraphScale.x(band.frequencyHz, plotWidth),
            inset + GraphScale.y(level(band, curve.preampDb), plotHeight, span),
        )
    }

    fun held(curve: EqualiserCurve, point: GraphPoint, reach: Float): Int? =
        curve.bands.indices
            .map { index -> index to handle(curve, index) }
            .filter { (_, centre) ->
                abs(centre.x - point.x) <= reach && abs(centre.y - point.y) <= reach
            }
            .minByOrNull { (_, centre) -> hypot(centre.x - point.x, centre.y - point.y) }
            ?.first

    fun placed(
        curve: EqualiserCurve,
        held: Int,
        point: GraphPoint,
        bounds: EqualiserBounds,
    ): EqualiserCurve {
        val band = curve.bands.getOrNull(held) ?: return curve
        val hertz =
            tidyHertz(GraphScale.hertzAt(point.x - left, plotWidth).coerceIn(bounds.hertz))
        val gain =
            if (band.filter.usesGain) {
                val drawn = GraphScale.dbAt(point.y - inset, plotHeight, span)
                tidyDb((drawn - curve.preampDb).coerceIn(bounds.gain))
            } else band.gainDb
        return curve.withBand(held, band.copy(frequencyHz = hertz, gainDb = gain))
    }
}

const val GRAPH_TALL_PER_WIDE = 10f / 16f

const val GRAPH_LOWEST_DP = 180f

const val GRAPH_HIGHEST_DP = 340f

fun graphHeightDp(widthDp: Float): Float =
    (widthDp * GRAPH_TALL_PER_WIDE).coerceIn(GRAPH_LOWEST_DP, GRAPH_HIGHEST_DP)

class LogScale(least: Float, most: Float) {
    private val low = max(least, 1e-6f)
    private val high = max(most, low * 1.001f)

    fun fraction(actual: Float): Float = (ln(actual.coerceIn(low, high) / low) / ln(high / low))

    fun actual(fraction: Float): Float = low * (high / low).pow(fraction.coerceIn(0f, 1f))
}

fun describeRoom(room: RoomCorrection?): String {
    if (room == null || room.channels == 0u) return ""
    val channels =
        when (room.channels) {
            1u -> "Mono"
            2u -> "Stereo"
            4u -> "Two-by-two (binaural)"
            else -> "${room.channels} channels"
        }
    val kilohertz = String.format(Locale.ROOT, "%.1f", room.rate.toDouble() / 1000).removeSuffix(".0")
    val seconds = String.format(Locale.ROOT, "%.2f", room.lengthMs.toDouble() / 1000)
    return "$channels · $kilohertz kHz · $seconds s"
}

data class Measured(val headphone: String, val source: String?)

fun measured(name: String): Measured {
    val parts = name.split(" — ", limit = 2)
    return Measured(parts[0].trim(), parts.getOrNull(1)?.trim()?.takeIf { it.isNotEmpty() })
}
