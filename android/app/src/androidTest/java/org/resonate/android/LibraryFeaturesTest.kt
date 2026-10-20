package org.resonate.android

import android.graphics.Bitmap
import android.graphics.Color
import android.os.ParcelFileDescriptor
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import java.io.ByteArrayOutputStream
import java.io.File
import java.nio.ByteBuffer
import java.nio.ByteOrder
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.resonate.android.nativecore.Documents
import org.resonate.android.nativecore.MobilePlayer
import org.resonate.android.nativecore.TrackOrder
import org.resonate.android.nativecore.Transport
import org.resonate.android.nativecore.defaultTrackFilter

@RunWith(AndroidJUnit4::class)
class LibraryFeaturesTest {
    @Test
    fun documentMetadataArtworkLyricsAnalysisStatisticsAndPlaylistsWorkTogether() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val folder =
            File(context.cacheDir, "library-features-${System.nanoTime()}").apply { mkdirs() }
        val wave = File(folder, "song.wav").apply { writeBytes(taggedTone()) }
        val documents =
            object : Documents {
                override fun openDocument(uri: String): Int =
                    ParcelFileDescriptor.open(wave, ParcelFileDescriptor.MODE_READ_ONLY).detachFd()
            }
        var player = MobilePlayer(folder.absolutePath, documents)
        try {
            val track = player.indexDocument("content://test/song.wav", "song.wav")
            assertEquals("Fixture song", track.title)
            assertEquals("Fixture artist", track.artist)
            assertEquals("Fixture album", track.album)
            assertEquals(2u, track.channels)
            val png =
                ByteArrayOutputStream().use { output ->
                    Bitmap.createBitmap(8, 8, Bitmap.Config.ARGB_8888)
                        .apply { eraseColor(Color.BLUE) }
                        .also { bitmap ->
                            bitmap.compress(Bitmap.CompressFormat.PNG, 100, output)
                            bitmap.recycle()
                        }
                    output.toByteArray()
                }
            player.setArtwork(track.id, png)
            assertArrayEquals(png, player.artwork(track.id))
            val lrc =
                "[ti:Fixture song]\n[ar:Fixture artist]\n[00:00.00]<00:00.00>Hello <00:01.00>world\n[00:02.00]Next line\n[00:04.00]Last line"
            player.keepSidecarLyrics(
                track.id,
                byteArrayOf(0xff.toByte(), 0xfe.toByte()) + lrc.toByteArray(Charsets.UTF_16LE),
            )
            val sheet = player.lyrics(track.id, false)
            assertEquals(3, sheet.lines.size)
            assertEquals("sidecar", sheet.origin)
            assertEquals(2, sheet.lines.first().words.size)
            assertEquals(1u, player.lyricFrame(track.id, 2500uL).line)
            assertTrue(player.lyricFrame(track.id, 2500uL).active.contains(1u))
            assertThrows(Exception::class.java) { player.searchOnline("Fixture song") }
            assertThrows(Exception::class.java) { player.recogniseTrack(track.id, "") }
            val report = player.analyseTrack(track.id)
            assertEquals(track.id, report.trackId)
            assertEquals(512, report.waveform.size)
            assertTrue(report.waveform.any { it > 0.001f })
            assertTrue(report.spectrum.isNotEmpty())
            assertTrue(report.clipped == 0uL)
            val inspector = player.inspector(track.id)
            assertTrue(inspector.seekable)
            assertEquals(track.uri, inspector.track.uri)
            val wanted =
                org.resonate.android.nativecore.SearchSong(
                    "",
                    track.title,
                    track.artist,
                    track.album,
                    track.durationMs,
                    emptyList(),
                )
            assertEquals(1uL, player.saveWanted(listOf(wanted)))
            assertTrue(player.wantedSongs(0u, 500u).single().held)
            val imported = player.importMatchedPlaylist("Shared collection", listOf(wanted, wanted))
            assertEquals(2u, imported.matched)
            assertEquals(0u, imported.missing)
            assertEquals(2, player.playlistTracks(imported.id, 0u, 500u).size)
            assertTrue(player.undoPlaylist())
            assertTrue(player.playlists().none { it.id == imported.id })
            val second = player.indexDocument("content://test/second.wav", "second.wav")
            val playlist =
                player.createPlaylist("Evening").also {
                    player.addToPlaylist(it, listOf(track.id, second.id, track.id))
                }
            assertEquals(3, player.playlistTracks(playlist, 0u, 100u).size)
            player.load(listOf(track.id, second.id, track.id), 0u, false)
            eventually {
                val state = player.snapshot()
                assertNull(state.failure)
                player.queue().size == 3 && state.current != null
            }
            assertNotEquals(player.queue()[0].queueId, player.queue()[2].queueId)
            player.setShuffle(true)
            eventually { player.snapshot().shuffle }
            val queue = player.queue()
            player.jumpTo(2u)
            eventually {
                player.snapshot().queueIndex == 2u && player.snapshot().current?.id == queue[2].id
            }
            player.pause()
            eventually { player.snapshot().transport == Transport.PAUSED }
            player.load(listOf(track.id), 0u, true)
            eventually {
                player.snapshot().transport == Transport.PLAYING &&
                    player.snapshot().positionMs > 200uL
            }
            eventually { player.visualizer(true).any { kotlin.math.abs(it) > 0.001f } }
            player.seek(6000uL)
            eventually { player.snapshot().positionMs >= 5900uL }
            assertEquals(0uL, player.statistics(0u).plays)
            player.seek(0uL)
            eventually { player.snapshot().positionMs < 1000uL }
            eventually { player.snapshot().positionMs > 4500uL }
            eventually { player.statistics(0u).plays == 1uL }
            assertTrue(player.statistics(0u).listenedMs >= 3500uL)
            player.favourite(track.id, true)
            assertEquals(
                1,
                player
                    .filteredTracks(
                        defaultTrackFilter().copy(favourites = true, order = TrackOrder.ALBUM),
                        0u,
                        100u,
                    )
                    .size,
            )
            player.pause()
            eventually { player.snapshot().transport == Transport.PAUSED }
            player.save()
            player.shutdown()
            player.close()
            player = MobilePlayer(folder.absolutePath, documents)
            assertArrayEquals(png, player.artwork(track.id))
            assertEquals(3, player.lyrics(track.id, false).lines.size)
            assertEquals(3, player.playlistTracks(playlist, 0u, 100u).size)
            assertTrue(player.filteredTracks(defaultTrackFilter().copy(text = "Fixture song"), 0u, 100u).any { it.favourite })
            eventually { player.statistics(0u).plays == 1uL }
            assertEquals(1, player.wantedSongs(0u, 500u).size)
        } finally {
            player.shutdown()
            player.close()
            folder.deleteRecursively()
        }
    }

    private fun taggedTone(): ByteArray {
        val info = ByteArrayOutputStream()
        info.write("INFO".toByteArray())
        listOf(
                "INAM" to "Fixture song",
                "IART" to "Fixture artist",
                "IPRD" to "Fixture album",
                "IGNR" to "Rock",
            )
            .forEach { (tag, value) ->
                val bytes = (value + '\u0000').toByteArray()
                info.write(tag.toByteArray())
                info.write(
                    ByteBuffer.allocate(4).order(ByteOrder.LITTLE_ENDIAN).putInt(bytes.size).array()
                )
                info.write(bytes)
                if (bytes.size % 2 == 1) info.write(0)
            }
        val base = pcmTone(8)
        val result =
            base +
                "LIST".toByteArray() +
                ByteBuffer.allocate(4).order(ByteOrder.LITTLE_ENDIAN).putInt(info.size()).array() +
                info.toByteArray()
        ByteBuffer.wrap(result).order(ByteOrder.LITTLE_ENDIAN).putInt(4, result.size - 8)
        return result
    }

    private fun eventually(condition: () -> Boolean) {
        val deadline = System.nanoTime() + 15_000_000_000L
        while (!condition()) {
            if (System.nanoTime() > deadline)
                throw AssertionError("Library feature condition did not settle")
            Thread.sleep(50)
        }
    }
}
