package org.resonate.android.ui.equaliser

import java.math.BigDecimal
import java.util.Locale
import org.resonate.android.R
import org.resonate.android.Words
import org.resonate.android.nativecore.EqualiserImport

class Measure(
    private val words: Words,
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
        get() =
            words.string(R.string.equaliser_reach, shown(range.start), shown(range.endInclusive))

    fun accepted(text: String): Float? = read(text)?.takeIf { it in range }
}

fun gainMeasure(words: Words, bounds: EqualiserBounds) =
    Measure(
        words = words,
        label = words.string(R.string.equaliser_gain),
        unit = words.string(R.string.equaliser_unit_decibels),
        range = bounds.gain,
        shown = { signedDb(words, it) },
        read = ::typedNumber,
        written = { plainNumber(tidyDb(it)) },
        signed = true,
        centred = true,
    )

fun hertzMeasure(words: Words, bounds: EqualiserBounds) =
    Measure(
        words = words,
        label = words.string(R.string.equaliser_frequency),
        unit = words.string(R.string.equaliser_unit_hertz),
        range = bounds.hertz,
        shown = { hertzLabel(words, it) },
        read = ::typedHertz,
        written = { plainNumber(tidyHertz(it)) },
        logarithmic = true,
    )

fun widthMeasure(words: Words, bounds: EqualiserBounds) =
    Measure(
        words = words,
        label = words.string(R.string.equaliser_width),
        unit = "",
        range = bounds.q,
        shown = { qLabel(words, it) },
        read = ::typedNumber,
        written = { plainNumber(tidyQ(it)) },
        logarithmic = true,
    )

fun preampMeasure(words: Words, bounds: EqualiserBounds) =
    Measure(
        words = words,
        label = words.string(R.string.equaliser_level),
        unit = words.string(R.string.equaliser_unit_decibels),
        range = bounds.preamp,
        shown = { signedDb(words, it) },
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

fun importSentence(words: Words, found: EqualiserImport): String {
    val count = found.curve.bands.size
    val bands = words.plural(R.plurals.equaliser_import_bands, count, count)
    val name = found.curve.name.ifBlank { words.string(R.string.equaliser_import_the_profile) }
    val loaded =
        if (found.converted) words.string(R.string.equaliser_import_loaded_fitted, bands, name)
        else words.string(R.string.equaliser_import_loaded, bands, name)
    val notes =
        listOfNotNull(
            found.clamped.takeIf { it > 0u }?.let {
                words.plural(R.plurals.equaliser_import_clamped, it.toInt(), it.toInt())
            },
            found.approximated.takeIf { it > 0u }?.let {
                words.plural(R.plurals.equaliser_import_approximated, it.toInt(), it.toInt())
            },
            found.passedOver.takeIf { it > 0u }?.let {
                words.plural(R.plurals.equaliser_import_passed_over, it.toInt(), it.toInt())
            },
        )
    val said = notes.joinToString("; ").replaceFirstChar { it.uppercase() }
    return loaded + if (said.isEmpty()) "" else " $said."
}
