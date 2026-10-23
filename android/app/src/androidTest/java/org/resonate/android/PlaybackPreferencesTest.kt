package org.resonate.android

import android.content.Context
import android.os.ParcelFileDescriptor
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import java.io.File
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.resonate.android.nativecore.Documents
import org.resonate.android.nativecore.EqualiserBand
import org.resonate.android.nativecore.EqualiserFilter
import org.resonate.android.nativecore.GainMode
import org.resonate.android.nativecore.LossyRestoration
import org.resonate.android.nativecore.MobilePlayer
import org.resonate.android.nativecore.ResamplingPhase
import org.resonate.android.nativecore.ResamplingQuality
import org.resonate.android.nativecore.SleepTimer
import org.resonate.android.nativecore.Transport
import org.resonate.android.nativecore.defaultPlaybackPreferences
import org.resonate.android.nativecore.tenBandCurve

@RunWith(AndroidJUnit4::class)
class PlaybackPreferencesTest {
    @Test
    fun soundChangesApplyDuringPlaybackAndSurviveReopening() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val name = "sound-test-${System.nanoTime()}"
        val folder = File(context.cacheDir, name).apply { mkdirs() }
        val wave = File(folder, "tone.wav").apply { writeBytes(pcmTone(30)) }
        val storage = context.getSharedPreferences(name, Context.MODE_PRIVATE)
        val store = PlaybackPreferencesStore(storage)
        val documents =
            object : Documents {
                override fun openDocument(uri: String): Int =
                    ParcelFileDescriptor.open(wave, ParcelFileDescriptor.MODE_READ_ONLY).detachFd()
            }
        var player = MobilePlayer(folder.absolutePath, documents)
        try {
            assertEquals(defaultPlaybackPreferences(), store.read())
            val track = player.indexDocument("content://test/tone.wav", "tone.wav")
            player.load(listOf(track.id), 0u, true)
            eventually { player.snapshot().transport == Transport.PLAYING }
            val settings =
                defaultPlaybackPreferences()
                    .copy(
                        equaliserEnabled = true,
                        equaliser = tenBandCurve(-6f, List(10) { if (it == 5) 6f else 0f }),
                        replayGain = GainMode.ALBUM,
                        quality = ResamplingQuality.BALANCED,
                        phase = ResamplingPhase.INTERMEDIATE,
                        truePeak = false,
                        restoration = LossyRestoration.REPAIR,
                    )
            player.configurePlayback(settings)
            store.write(settings)
            eventually { player.snapshot().positionMs > 1500uL }
            assertEquals(Transport.PLAYING, player.snapshot().transport)
            assertNull(player.snapshot().failure)
            assertThrows(Exception::class.java) {
                player.configurePlayback(
                    settings.copy(
                        equaliser =
                            settings.equaliser.copy(
                                bands = listOf(EqualiserBand(EqualiserFilter.PEAKING, 10f, 0f, 1f))
                            )
                    )
                )
            }
            assertEquals(settings, player.playbackPreferences())
            player.pause()
            eventually { player.snapshot().transport == Transport.PAUSED }
            player.setSleep(SleepTimer.EndOfTrack)
            eventually { player.snapshot().sleep == SleepTimer.EndOfTrack }
            player.setSleep(SleepTimer.EndOfQueue)
            eventually { player.snapshot().sleep == SleepTimer.EndOfQueue }
            player.setSleep(null)
            eventually { player.snapshot().sleep == null }
            player.shutdown()
            player.close()
            player = MobilePlayer(folder.absolutePath, documents)
            player.configurePlayback(store.read())
            val restored = player.playbackPreferences()
            assertEquals(settings.copy(equaliser = restored.equaliser), restored)
            assertEquals(
                settings.equaliser.copy(bands = restored.equaliser.bands),
                restored.equaliser,
            )
            assertEquals(settings.equaliser.bands.size, restored.equaliser.bands.size)
            settings.equaliser.bands.zip(restored.equaliser.bands).forEach { (before, after) ->
                assertEquals(before.copy(q = after.q), after)
                assertEquals(before.q, after.q, 0.001f)
            }
            eventually { player.snapshot().current != null }
            assertEquals(Transport.PAUSED, player.snapshot().transport)
            storage
                .edit()
                .putString("quality", "unknown-future-quality")
                .remove("equaliser-curve")
                .commit()
            assertEquals(ResamplingQuality.HIGH, store.read().quality)
            assertEquals(defaultPlaybackPreferences().equaliser, store.read().equaliser)
            assertTrue(store.read().equaliserEnabled)
        } finally {
            player.shutdown()
            player.close()
            context.deleteSharedPreferences(name)
            folder.deleteRecursively()
        }
    }

    private fun eventually(condition: () -> Boolean) {
        val deadline = System.nanoTime() + 15_000_000_000L
        while (!condition()) {
            if (System.nanoTime() > deadline) throw AssertionError("Sound settings did not settle")
            Thread.sleep(50)
        }
    }
}
