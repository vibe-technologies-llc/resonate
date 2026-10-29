package org.resonate.android

import android.content.Context
import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioManager
import android.media.AudioRecord
import android.media.AudioTrack
import android.media.MediaRecorder
import java.io.IOException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.withContext
import org.resonate.android.nativecore.MeasurementSweep

object SweepRecorder {
    private const val CHUNKS_A_SECOND = 10

    fun outputRate(): UInt = AudioTrack.getNativeOutputSampleRate(AudioManager.STREAM_MUSIC).toUInt()

    suspend fun measure(context: Context, sweep: MeasurementSweep): FloatArray =
        withContext(Dispatchers.IO) {
            val rate = sweep.rate.toInt()
            val wanted = sweep.recordedFrames.toInt()
            val chunk = rate / CHUNKS_A_SECOND
            val record = openRecord(context, rate, chunk)
            var track: AudioTrack? = null
            try {
                if (record.state != AudioRecord.STATE_INITIALIZED)
                    throw IOException("The microphone could not be opened")
                val played = openTrack(rate, sweep.samples)
                track = played
                val samples = FloatArray(wanted)
                record.startRecording()
                played.play()
                var filled = 0
                while (filled < wanted) {
                    ensureActive()
                    val read =
                        record.read(
                            samples,
                            filled,
                            minOf(chunk, wanted - filled),
                            AudioRecord.READ_BLOCKING,
                        )
                    if (read < 0) throw IOException("The microphone stopped answering")
                    filled += read
                }
                samples
            } finally {
                runCatching { track?.stop() }
                track?.release()
                runCatching { record.stop() }
                record.release()
            }
        }

    private fun openRecord(context: Context, rate: Int, chunk: Int): AudioRecord {
        val minimum =
            AudioRecord.getMinBufferSize(
                rate,
                AudioFormat.CHANNEL_IN_MONO,
                AudioFormat.ENCODING_PCM_FLOAT,
            )
        if (minimum <= 0) throw IOException("This phone has no microphone Resonate can use")
        val manager = context.getSystemService(AudioManager::class.java)
        val unprocessed =
            manager.getProperty(AudioManager.PROPERTY_SUPPORT_AUDIO_SOURCE_UNPROCESSED) == "true"
        val source =
            if (unprocessed) MediaRecorder.AudioSource.UNPROCESSED else MediaRecorder.AudioSource.MIC
        return try {
            AudioRecord.Builder()
                .setAudioSource(source)
                .setAudioFormat(
                    AudioFormat.Builder()
                        .setEncoding(AudioFormat.ENCODING_PCM_FLOAT)
                        .setSampleRate(rate)
                        .setChannelMask(AudioFormat.CHANNEL_IN_MONO)
                        .build()
                )
                .setBufferSizeInBytes(maxOf(minimum, chunk * Float.SIZE_BYTES * 2))
                .build()
        } catch (refused: SecurityException) {
            throw IOException("Microphone access was refused", refused)
        }
    }

    private fun openTrack(rate: Int, samples: List<Float>): AudioTrack {
        val track =
            AudioTrack.Builder()
                .setAudioAttributes(
                    AudioAttributes.Builder()
                        .setUsage(AudioAttributes.USAGE_MEDIA)
                        .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
                        .build()
                )
                .setAudioFormat(
                    AudioFormat.Builder()
                        .setEncoding(AudioFormat.ENCODING_PCM_FLOAT)
                        .setSampleRate(rate)
                        .setChannelMask(AudioFormat.CHANNEL_OUT_MONO)
                        .build()
                )
                .setBufferSizeInBytes(samples.size * Float.SIZE_BYTES)
                .setTransferMode(AudioTrack.MODE_STATIC)
                .build()
        val written = track.write(samples.toFloatArray(), 0, samples.size, AudioTrack.WRITE_BLOCKING)
        if (written != samples.size) {
            track.release()
            throw IOException("The sweep could not be sent to the speaker")
        }
        return track
    }
}
