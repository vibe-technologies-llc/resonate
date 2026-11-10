package org.resonate.android.ui.settings

import android.media.AudioFormat
import org.resonate.android.AudioOutput
import org.resonate.android.R
import org.resonate.android.Words
import org.resonate.android.namedByType
import org.resonate.android.nativecore.OutputEncoding
import org.resonate.android.nativecore.OutputPath
import org.resonate.android.nativecore.OutputStream
import org.resonate.android.oneForEachRoute
import org.resonate.android.outputName

const val SYSTEM_DEFAULT_OUTPUT = 0

val systemDefaultOutput =
    AudioOutput(SYSTEM_DEFAULT_OUTPUT, "", "", emptyList(), emptyList(), emptyList())

val AudioOutput.isSystemDefault: Boolean
    get() = id == SYSTEM_DEFAULT_OUTPUT

fun outputTitle(words: Words, output: AudioOutput): String =
    if (output.isSystemDefault) words.string(R.string.output_system_default)
    else outputName(output.type, output.kind, output.name)

fun outputSubtitle(words: Words, output: AudioOutput): String =
    when {
        output.isSystemDefault -> words.string(R.string.output_system_default_detail)
        output.type in namedByType -> output.name
        else -> output.kind
    }

private fun kilohertz(rate: Int): String {
    val khz = rate / 1000.0
    return if (khz % 1.0 == 0.0) "${khz.toInt()}" else "%.1f".format(java.util.Locale.ROOT, khz)
}

fun outputRates(words: Words, output: AudioOutput): String? {
    val rates = output.rates.filter { it > 0 }.distinct().sorted()
    if (rates.isEmpty()) return null
    val shown = rates.map(::kilohertz)
    return if (shown.size == 1) words.string(R.string.unit_kilohertz, shown.first())
    else words.string(R.string.output_rates_range, shown.first(), shown.last())
}

private fun bitsOf(encoding: Int): Int? =
    when (encoding) {
        AudioFormat.ENCODING_PCM_8BIT -> 8
        AudioFormat.ENCODING_PCM_16BIT -> 16
        AudioFormat.ENCODING_PCM_24BIT_PACKED -> 24
        AudioFormat.ENCODING_PCM_32BIT -> 32
        else -> null
    }

fun outputDepths(words: Words, output: AudioOutput): String? {
    val bits = output.encodings.mapNotNull(::bitsOf).distinct().sorted()
    val float = AudioFormat.ENCODING_PCM_FLOAT in output.encodings
    val shown =
        listOfNotNull(
            bits
                .takeIf { it.isNotEmpty() }
                ?.let { words.string(R.string.output_depth_bits, it.joinToString(", ")) },
            words.string(R.string.output_depth_float).takeIf { float },
        )
    return shown.takeIf { it.isNotEmpty() }?.joinToString(", ")
}

fun outputChannels(words: Words, output: AudioOutput): String? {
    val counts = output.channels.filter { it > 0 }.distinct().sorted()
    if (counts.isEmpty()) return null
    return if (counts.size == 1)
        words.plural(R.plurals.output_channels, counts.first(), counts.first())
    else
        words.plural(
            R.plurals.output_channels_range,
            counts.last(),
            counts.first(),
            counts.last(),
        )
}

fun outputTakes(words: Words, output: AudioOutput): String? =
    listOfNotNull(
            outputRates(words, output),
            outputDepths(words, output),
            outputChannels(words, output),
        )
        .takeIf { it.isNotEmpty() }
        ?.joinToString(" · ")

fun outputStreamWords(words: Words, stream: OutputStream): String {
    val depth =
        when (stream.format.encoding) {
            OutputEncoding.PCM16 -> words.string(R.string.output_depth_16)
            OutputEncoding.PCM32 -> words.string(R.string.output_depth_32)
            OutputEncoding.FLOAT -> words.string(R.string.output_depth_32_float)
        }
    val layout =
        if (stream.format.channels == 1u) words.string(R.string.output_mono)
        else words.string(R.string.output_stereo)
    val rate = words.string(R.string.unit_kilohertz, kilohertz(stream.format.rate.toInt()))
    return listOf(rate, depth, layout).joinToString(" · ")
}

fun outputPathWords(words: Words, path: OutputPath): String =
    when (path) {
        OutputPath.MIXED -> words.string(R.string.output_path_mixed)
        OutputPath.EXCLUSIVE -> words.string(R.string.output_path_exclusive)
        OutputPath.BIT_PERFECT -> words.string(R.string.output_path_bit_perfect)
    }

fun clampedSamplesWords(count: ULong): String? = count.takeIf { it > 0uL }?.toString()

fun withSystemDefault(outputs: List<AudioOutput>): List<AudioOutput> =
    listOf(systemDefaultOutput) + outputs.filter { it.id != SYSTEM_DEFAULT_OUTPUT }

fun offeredOutputs(outputs: List<AudioOutput>): List<AudioOutput> =
    withSystemDefault(oneForEachRoute(outputs))

fun standsFor(offered: AudioOutput, id: Int, outputs: List<AudioOutput>): Boolean =
    offered.id == id ||
        offered.route != null &&
            outputs.any { it.id == id && it.route?.id == offered.route.id }

fun outputNamed(words: Words, outputs: List<AudioOutput>, id: Int): String =
    withSystemDefault(outputs).firstOrNull { it.id == id }?.let { outputTitle(words, it) }
        ?: words.string(R.string.output_unavailable)

fun pendingOutput(selected: Int, actual: Int): Int? =
    selected.takeIf { it != SYSTEM_DEFAULT_OUTPUT && it != actual }
