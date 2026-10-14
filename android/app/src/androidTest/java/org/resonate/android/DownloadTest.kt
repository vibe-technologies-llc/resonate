package org.resonate.android

import android.os.ParcelFileDescriptor
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import java.io.File
import java.net.InetAddress
import java.net.ServerSocket
import java.util.concurrent.Executors
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test
import org.junit.runner.RunWith
import org.resonate.android.nativecore.AndroidPlayer
import org.resonate.android.nativecore.Documents
import org.resonate.android.nativecore.DownloadAccount
import org.resonate.android.nativecore.MusicProvider
import org.resonate.android.nativecore.SearchSong

@RunWith(AndroidJUnit4::class)
class DownloadTest {
    @Test
    fun providerBytesReachOnlyTheDetachedDestinationAndCancellationStaysCancelled() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val folder = File(context.cacheDir, "download-test-${System.nanoTime()}").apply { mkdirs() }
        val destination = File(folder, "delivered.wav").apply { createNewFile() }
        val audio = pcmTone(8)
        val recording = "83d91898-7763-47d7-b03b-b92132375c47"
        val server = ServerSocket(0, 8, InetAddress.getByName("127.0.0.1"))
        val worker = Executors.newSingleThreadExecutor()
        val serving = worker.submit {
            try {
                while (!server.isClosed) server.accept().use { socket ->
                    socket.soTimeout = 5000
                    val reader = socket.getInputStream().bufferedReader(Charsets.US_ASCII)
                    val request = reader.readLine().orEmpty()
                    while (!reader.readLine().isNullOrEmpty()) {}
                    val search = request.contains("/search3?")
                    val body =
                        if (search)
                            """{"subsonic-response":{"status":"ok","version":"1.16.1","searchResult3":{"song":[{"id":"fixture","title":"Fixture song","artist":"Fixture artist","suffix":"wav","duration":8,"musicBrainzId":"$recording"}]}}}"""
                                .toByteArray()
                        else audio
                    val type = if (search) "application/json" else "audio/wav"
                    socket.getOutputStream().apply {
                        write(
                            "HTTP/1.1 200 OK\r\nContent-Type: $type\r\nContent-Length: ${body.size}\r\nConnection: close\r\n\r\n"
                                .toByteArray(Charsets.US_ASCII)
                        )
                        write(body)
                        flush()
                    }
                }
            } catch (closed: java.net.SocketException) {
                if (!server.isClosed) throw closed
            }
        }
        val player =
            AndroidPlayer(
                folder.absolutePath,
                object : Documents {
                    override fun open(uri: String) =
                        ParcelFileDescriptor.open(destination, ParcelFileDescriptor.MODE_READ_ONLY)
                            .detachFd()
                },
            )
        try {
            player.configureOnline(true)
            val account =
                DownloadAccount(
                    MusicProvider.SUBSONIC,
                    "http://127.0.0.1:${server.localPort}",
                    "fixture",
                    "fixture",
                    "",
                    "",
                    "",
                )
            val song =
                SearchSong(recording, "Fixture song", "Fixture artist", "", 8000uL, emptyList())
            val offer = requireNotNull(player.findDownload(song, account))
            val descriptor =
                ParcelFileDescriptor.open(destination, ParcelFileDescriptor.MODE_WRITE_ONLY)
                    .detachFd()
            assertEquals(audio.size.toULong(), player.downloadOffer(offer.id, descriptor))
            assertEquals(audio.size.toULong(), player.downloadBytes())
            assertArrayEquals(audio, destination.readBytes())
            player.validateDownload("content://test/delivered.wav", 8000uL)
            player.saveWanted(listOf(song))
            val track = player.indexDownload("content://test/delivered.wav", "delivered.wav", song)
            assertEquals(song.title, track.title)
            assertEquals(song.artist, track.artist)
            assertEquals(recording, track.recording)
            assertEquals(true, player.wantedSongs(0u, 500u).single().held)
            val rescanned = player.indexDocument("content://test/delivered.wav", "delivered.wav")
            assertEquals(track.id, rescanned.id)
            assertEquals(song.title, rescanned.title)
            assertEquals(song.artist, rescanned.artist)
            assertThrows(Exception::class.java) {
                player.validateDownload("content://test/delivered.wav", 60000uL)
            }
            val cancelled = requireNotNull(player.findDownload(song, account))
            player.cancelDownload()
            val empty = File(folder, "cancelled.wav").apply { createNewFile() }
            val cancelledDescriptor =
                ParcelFileDescriptor.open(empty, ParcelFileDescriptor.MODE_WRITE_ONLY).detachFd()
            assertThrows(Exception::class.java) {
                player.downloadOffer(cancelled.id, cancelledDescriptor)
            }
            assertEquals(0L, empty.length())
        } finally {
            player.shutdown()
            player.close()
            server.close()
            serving.get(5, java.util.concurrent.TimeUnit.SECONDS)
            worker.shutdownNow()
            folder.deleteRecursively()
        }
    }
}
