package org.resonate.android.ui.settings

import org.resonate.android.AudioOutput
import org.resonate.android.outputName

const val SYSTEM_DEFAULT_OUTPUT = 0

const val AUTOMATIC_KIND = "Automatic"

val systemDefaultOutput = AudioOutput(SYSTEM_DEFAULT_OUTPUT, "System default", AUTOMATIC_KIND, emptyList(), emptyList(), emptyList())

private val namedByKind = setOf("Built-in speaker", "Earpiece")

fun outputTitle(output: AudioOutput): String =
    if (output.kind == AUTOMATIC_KIND) output.name else outputName(output.kind, output.name)

fun outputSubtitle(output: AudioOutput): String =
    when {
        output.kind == AUTOMATIC_KIND -> "Android chooses for you"
        output.kind in namedByKind -> output.name
        else -> output.kind
    }

fun outputRates(output: AudioOutput): String? {
    val rates = output.rates.filter { it > 0 }.distinct().sorted()
    if (rates.isEmpty()) return null
    val shown = rates.map { rate ->
        val khz = rate / 1000.0
        if (khz % 1.0 == 0.0) "${khz.toInt()}" else "%.1f".format(java.util.Locale.ROOT, khz)
    }
    return if (shown.size == 1) "${shown.first()} kHz"
    else "${shown.first()}–${shown.last()} kHz"
}

const val CLAMPED_SAMPLES_NOTE =
    "Samples past full scale, held at it since this stream opened"

fun clampedSamplesWords(count: ULong): String? = count.takeIf { it > 0uL }?.toString()

fun withSystemDefault(outputs: List<AudioOutput>): List<AudioOutput> =
    listOf(systemDefaultOutput) + outputs.filter { it.id != SYSTEM_DEFAULT_OUTPUT }

fun outputNamed(outputs: List<AudioOutput>, id: Int): String =
    withSystemDefault(outputs).firstOrNull { it.id == id }?.let(::outputTitle)
        ?: "An unavailable device"

fun pendingOutput(selected: Int, actual: Int): Int? =
    selected.takeIf { it != SYSTEM_DEFAULT_OUTPUT && it != actual }
