package org.resonate.android

import android.content.Context
import android.media.AudioAttributes
import android.media.AudioDeviceInfo
import android.media.AudioFormat
import android.media.AudioManager
import android.media.AudioMixerAttributes
import android.os.Build
import android.util.Log
import org.resonate.android.nativecore.OutputEncoding
import org.resonate.android.nativecore.OutputFormat
import org.resonate.android.nativecore.OutputMixer

fun outputEncodingOf(encoding: Int): OutputEncoding? =
    when (encoding) {
        AudioFormat.ENCODING_PCM_16BIT -> OutputEncoding.PCM16
        AudioFormat.ENCODING_PCM_32BIT -> OutputEncoding.PCM32
        AudioFormat.ENCODING_PCM_FLOAT -> OutputEncoding.FLOAT
        else -> null
    }

fun outputFormatOf(encoding: Int, rate: Int, channels: Int): OutputFormat? {
    val named = outputEncodingOf(encoding) ?: return null
    if (rate <= 0 || channels !in 1..2) return null
    return OutputFormat(rate.toUInt(), named, channels.toUInt())
}

fun matchesFormat(format: AudioFormat, wanted: OutputFormat): Boolean =
    outputFormatOf(format.encoding, format.sampleRate, format.channelCount) == wanted

internal fun mediaAttributes(): AudioAttributes =
    AudioAttributes.Builder()
        .setUsage(AudioAttributes.USAGE_MEDIA)
        .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
        .build()

private fun bitPerfectAttributes(
    manager: AudioManager,
    device: AudioDeviceInfo,
): List<AudioMixerAttributes> =
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.UPSIDE_DOWN_CAKE) emptyList()
    else
        try {
            manager.getSupportedMixerAttributes(device).filter {
                it.mixerBehavior == AudioMixerAttributes.MIXER_BEHAVIOR_BIT_PERFECT
            }
        } catch (failure: RuntimeException) {
            Log.w("Resonate", "The output's mixer could not be asked what it takes", failure)
            emptyList()
        }

fun bitPerfectFormats(manager: AudioManager, device: AudioDeviceInfo): List<OutputFormat> =
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.UPSIDE_DOWN_CAKE) emptyList()
    else
        bitPerfectAttributes(manager, device)
            .mapNotNull {
                outputFormatOf(it.format.encoding, it.format.sampleRate, it.format.channelCount)
            }
            .distinct()

class BitPerfectMixer(private val context: Context) : OutputMixer {
    private val manager: AudioManager?
        get() = context.getSystemService(AudioManager::class.java)

    private fun deviceOf(manager: AudioManager, id: Int): AudioDeviceInfo? =
        manager.getDevices(AudioManager.GET_DEVICES_OUTPUTS).firstOrNull { it.id == id }

    override fun preferBitPerfect(device: Int, format: OutputFormat): Boolean {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.UPSIDE_DOWN_CAKE) return false
        val manager = manager ?: return false
        val output = deviceOf(manager, device) ?: return false
        val attributes =
            bitPerfectAttributes(manager, output).firstOrNull { matchesFormat(it.format, format) }
                ?: return false
        return try {
            manager.setPreferredMixerAttributes(mediaAttributes(), output, attributes)
        } catch (failure: RuntimeException) {
            Log.w("Resonate", "Android would not take the bit-perfect mixer", failure)
            false
        }
    }

    override fun mixFreely(device: Int) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.UPSIDE_DOWN_CAKE) return
        val manager = manager ?: return
        val output = deviceOf(manager, device) ?: return
        try {
            manager.clearPreferredMixerAttributes(mediaAttributes(), output)
        } catch (failure: RuntimeException) {
            Log.w("Resonate", "Android would not give the mixer back", failure)
        }
    }
}
