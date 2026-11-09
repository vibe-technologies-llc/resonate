package org.resonate.android.ui.equaliser

import java.math.BigDecimal
import java.util.Locale
import org.resonate.android.nativecore.EqualiserImport

class Measure(
    val label: String,
    val unit: String,
    val range: ClosedFloatingPointRange<Float>,
    val shown: (Float) -> String,
    val read: (String) -> Float?,
    val written: (Float) -> String,
    val signed: Boolean = false,
    val logarithmic: Boolean = false,
    val centred: Boolean = false,
) {
    val reach: String
        get() = "From ${shown(range.start)} to ${shown(range.endInclusive)}"

    fun accepted(text: String): Float? = read(text)?.takeIf { it in range }
}

fun gainMeasure(bounds: EqualiserBounds) =
    Measure(
        label = "Gain",
        unit = "dB",
        range = bounds.gain,
        shown = ::signedDb,
        read = ::typedNumber,
        written = { plainNumber(tidyDb(it)) },
        signed = true,
        centred = true,
    )

fun hertzMeasure(bounds: EqualiserBounds) =
    Measure(
        label = "Frequency",
        unit = "Hz",
        range = bounds.hertz,
        shown = ::hertzLabel,
        read = ::typedHertz,
        written = { plainNumber(tidyHertz(it)) },
        logarithmic = true,
    )

fun widthMeasure(bounds: EqualiserBounds) =
    Measure(
        label = "Width",
        unit = "",
        range = bounds.q,
        shown = ::qLabel,
        read = ::typedNumber,
        written = { plainNumber(tidyQ(it)) },
        logarithmic = true,
    )

fun preampMeasure(bounds: EqualiserBounds) =
    Measure(
        label = "Level",
        unit = "dB",
        range = bounds.preamp,
        shown = ::signedDb,
        read = ::typedNumber,
        written = { plainNumber(tidyDb(it)) },
        signed = true,
        centred = true,
    )

private val HERTZ_WORDS = listOf("khz", "hz", "k")

private const val DECIBELS = "db"

private fun cleaned(text: String): String =
    text.trim().lowercase(Locale.ROOT).replace(',', '.').replace('−', '-').replace(" ", "")

private fun numberIn(text: String): Float? =
    text.removePrefix("q").removePrefix("+").toFloatOrNull()?.takeIf { it.isFinite() }

fun typedNumber(text: String): Float? = numberIn(cleaned(text).removeSuffix(DECIBELS))

fun typedHertz(text: String): Float? {
    val plain = cleaned(text)
    val kilo = plain.endsWith("khz") || plain.endsWith("k")
    val unit = HERTZ_WORDS.firstOrNull { plain.endsWith(it) }
    val number = numberIn(if (unit == null) plain else plain.removeSuffix(unit)) ?: return null
    return if (kilo) number * 1000f else number
}

fun flippedSign(text: String): String {
    val plain = text.trim()
    return when {
        plain.startsWith("-") || plain.startsWith("−") -> plain.drop(1)
        plain.startsWith("+") -> "-" + plain.drop(1)
        else -> "-$plain"
    }
}

fun plainNumber(value: Float): String =
    BigDecimal(value.toString()).stripTrailingZeros().toPlainString().let {
        if (it == "-0") "0" else it
    }

fun counted(count: UInt, one: String, many: String): String =
    if (count == 1u) "1 $one" else "$count $many"

fun importSentence(found: EqualiserImport): String {
    val bands = counted(found.curve.bands.size.toUInt(), "band", "bands")
    val name = found.curve.name.ifBlank { "the profile" }
    val fitted = if (found.converted) ", fitted from its GraphicEQ line" else ""
    val notes =
        listOfNotNull(
            found.clamped.takeIf { it > 0u }?.let {
                "${counted(it, "value was", "values were")} held to the equaliser's reach"
            },
            found.approximated.takeIf { it > 0u }?.let {
                "${counted(it, "filter was", "filters were")} approximated"
            },
            found.passedOver.takeIf { it > 0u }?.let {
                "${counted(it, "line was", "lines were")} passed over"
            },
        )
    val said = notes.joinToString("; ").replaceFirstChar { it.uppercase() }
    return "Loaded $bands from $name$fitted." + if (said.isEmpty()) "" else " $said."
}
