package org.resonate.android.ui.settings

import android.media.AudioFormat
import org.resonate.android.AudioOutput
import org.resonate.android.nativecore.OutputEncoding
import org.resonate.android.nativecore.OutputPath
import org.resonate.android.nativecore.OutputStream
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

private fun kilohertz(rate: Int): String {
    val khz = rate / 1000.0
    return if (khz % 1.0 == 0.0) "${khz.toInt()}" else "%.1f".format(java.util.Locale.ROOT, khz)
}

fun outputRates(output: AudioOutput): String? {
    val rates = output.rates.filter { it > 0 }.distinct().sorted()
    if (rates.isEmpty()) return null
    val shown = rates.map(::kilohertz)
    return if (shown.size == 1) "${shown.first()} kHz"
    else "${shown.first()}–${shown.last()} kHz"
}

private fun bitsOf(encoding: Int): Int? =
    when (encoding) {
        AudioFormat.ENCODING_PCM_8BIT -> 8
        AudioFormat.ENCODING_PCM_16BIT -> 16
        AudioFormat.ENCODING_PCM_24BIT_PACKED -> 24
        AudioFormat.ENCODING_PCM_32BIT -> 32
        else -> null
    }

fun outputDepths(output: AudioOutput): String? {
    val bits = output.encodings.mapNotNull(::bitsOf).distinct().sorted()
    val float = AudioFormat.ENCODING_PCM_FLOAT in output.encodings
    val words =
        listOfNotNull(
            bits.takeIf { it.isNotEmpty() }?.joinToString(", ", postfix = "-bit"),
            "float".takeIf { float },
        )
    return words.takeIf { it.isNotEmpty() }?.joinToString(", ")
}

fun outputChannels(output: AudioOutput): String? {
    val counts = output.channels.filter { it > 0 }.distinct().sorted()
    if (counts.isEmpty()) return null
    return if (counts.size == 1) "${counts.first()} channels"
    else "${counts.first()}–${counts.last()} channels"
}

fun outputTakes(output: AudioOutput): String? =
    listOfNotNull(outputRates(output), outputDepths(output), outputChannels(output))
        .takeIf { it.isNotEmpty() }
        ?.joinToString(" · ")

fun outputStreamWords(stream: OutputStream): String {
    val depth =
        when (stream.format.encoding) {
            OutputEncoding.PCM16 -> "16-bit"
            OutputEncoding.PCM32 -> "32-bit"
            OutputEncoding.FLOAT -> "32-bit float"
        }
    val layout = if (stream.format.channels == 1u) "mono" else "stereo"
    return "${kilohertz(stream.format.rate.toInt())} kHz · $depth · $layout"
}

fun outputPathWords(path: OutputPath): String =
    when (path) {
        OutputPath.MIXED -> "Mixed by Android"
        OutputPath.EXCLUSIVE -> "Exclusive: Android's mixer is passed by"
        OutputPath.BIT_PERFECT -> "Bit-perfect: the USB mixer hands it on untouched"
    }

const val FOLLOW_SONG_RATE_NOTE =
    "Opens the output at the song's own rate and depth where this output takes it unmixed: an exclusive stream, or a USB DAC's bit-perfect mode on Android 14 and later. Elsewhere Android's mixer plays it at 48 kHz. An exclusive stream wakes the phone more often."

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
