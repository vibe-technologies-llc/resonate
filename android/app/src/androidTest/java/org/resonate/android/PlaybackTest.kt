package org.resonate.android

import android.os.ParcelFileDescriptor
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import java.io.File
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.resonate.android.nativecore.Documents
import org.resonate.android.nativecore.MobilePlayer
import org.resonate.android.nativecore.Transport

@RunWith(AndroidJUnit4::class)
class PlaybackTest {
    @Test
    fun grantedDocumentPlaysPausesSeeksAndResumes() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val folder = File(context.cacheDir, "playback-test-${System.nanoTime()}").apply { mkdirs() }
        val wave = File(folder, "tone.wav").apply { writeBytes(pcmTone()) }
        val documents =
            object : Documents {
                override fun openDocument(uri: String): Int {
                    assertEquals("content://test/tree/music%3A/document/tone.wav", uri)
                    return ParcelFileDescriptor.open(wave, ParcelFileDescriptor.MODE_READ_ONLY)
                        .detachFd()
                }
            }
        var player = MobilePlayer(folder.absolutePath, documents)
        try {
            val track =
                player.indexDocument("content://test/tree/music%3A/document/tone.wav", "tone.wav")
            assertEquals(44100u, track.rate)
            assertTrue(track.durationMs >= 9900uL)
            assertEquals(1, player.tracks("tone", 0u, 100u).size)
            player.load(listOf(track.id), 0u, true)
            eventually { player.snapshot().transport == Transport.PLAYING }
            eventually { player.snapshot().positionMs > 300uL }
            assertEquals(48000u, player.snapshot().outputRate)
            player.pause()
            eventually { player.snapshot().transport == Transport.PAUSED }
            player.seek(3000uL)
            eventually { player.snapshot().positionMs >= 2900uL }
            player.favourite(track.id, true)
            player.save()
            player.shutdown()
            player.close()
            player = MobilePlayer(folder.absolutePath, documents)
            eventually { player.snapshot().current != null }
            assertEquals(Transport.PAUSED, player.snapshot().transport)
            assertTrue(player.snapshot().positionMs >= 2900uL)
            assertTrue(player.tracks("", 0u, 100u).single().favourite)
        } finally {
            player.shutdown()
            player.close()
            folder.deleteRecursively()
        }
    }

    private fun eventually(condition: () -> Boolean) {
        val deadline = System.nanoTime() + 15_000_000_000L
        while (!condition()) {
            if (System.nanoTime() > deadline)
                throw AssertionError("Playback condition did not settle")
            Thread.sleep(50)
        }
    }
}
