package org.resonate.android

import java.nio.ByteBuffer
import java.nio.ByteOrder
import kotlin.math.PI
import kotlin.math.sin

internal fun pcmTone(seconds: Int = 10): ByteArray {
    val frames = 44100 * seconds
    val dataSize = frames * 4
    val buffer = ByteBuffer.allocate(44 + dataSize).order(ByteOrder.LITTLE_ENDIAN)
    buffer.put("RIFF".toByteArray()).putInt(36 + dataSize).put("WAVEfmt ".toByteArray())
    buffer
        .putInt(16)
        .putShort(1)
        .putShort(2)
        .putInt(44100)
        .putInt(44100 * 4)
        .putShort(4)
        .putShort(16)
    buffer.put("data".toByteArray()).putInt(dataSize)
    repeat(frames) { frame ->
        val value = (sin(frame * 2 * PI * 440 / 44100) * 1500).toInt().toShort()
        buffer.putShort(value).putShort(value)
    }
    return buffer.array()
}
