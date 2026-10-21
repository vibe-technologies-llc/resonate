package org.resonate.android

import android.media.AudioFormat
import android.media.AudioRecord
import android.media.MediaRecorder
import java.io.IOException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.withContext

class HeardClip(val samples: FloatArray, val rate: Int)

object Microphone {
    const val RATE = 44_100
    private const val CHUNK_FRAMES = RATE / 10

    suspend fun record(seconds: Int): HeardClip =
        withContext(Dispatchers.IO) {
            val wanted = RATE * seconds
            val minimum =
                AudioRecord.getMinBufferSize(
                    RATE,
                    AudioFormat.CHANNEL_IN_MONO,
                    AudioFormat.ENCODING_PCM_FLOAT,
                )
            if (minimum <= 0) throw IOException("This phone has no microphone Resonate can use")
            val record =
                try {
                    AudioRecord.Builder()
                        .setAudioSource(MediaRecorder.AudioSource.MIC)
                        .setAudioFormat(
                            AudioFormat.Builder()
                                .setEncoding(AudioFormat.ENCODING_PCM_FLOAT)
                                .setSampleRate(RATE)
                                .setChannelMask(AudioFormat.CHANNEL_IN_MONO)
                                .build()
                        )
                        .setBufferSizeInBytes(maxOf(minimum, CHUNK_FRAMES * Float.SIZE_BYTES * 2))
                        .build()
                } catch (refused: SecurityException) {
                    throw IOException("Microphone access was refused", refused)
                }
            try {
                if (record.state != AudioRecord.STATE_INITIALIZED)
                    throw IOException("The microphone could not be opened")
                val samples = FloatArray(wanted)
                record.startRecording()
                var filled = 0
                while (filled < wanted) {
                    ensureActive()
                    val read =
                        record.read(
                            samples,
                            filled,
                            minOf(CHUNK_FRAMES, wanted - filled),
                            AudioRecord.READ_BLOCKING,
                        )
                    if (read < 0) throw IOException("The microphone stopped answering")
                    filled += read
                }
                HeardClip(samples, RATE)
            } finally {
                record.release()
            }
        }
}
