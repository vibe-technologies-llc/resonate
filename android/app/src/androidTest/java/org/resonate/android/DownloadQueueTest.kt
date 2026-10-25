package org.resonate.android

import android.net.Uri
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import java.net.InetAddress
import java.net.ServerSocket
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.resonate.android.nativecore.DownloadAccount
import org.resonate.android.nativecore.MusicProvider
import org.resonate.android.nativecore.SearchSong

@RunWith(AndroidJUnit4::class)
class DownloadQueueTest {
    private val recordings =
        listOf(
            "11111111-1111-4111-8111-111111111111",
            "22222222-2222-4222-8222-222222222222",
            "33333333-3333-4333-8333-333333333333",
        )

    @Test
    fun severalSongsDownloadTwoAtATimeAndAllLand() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val audio = pcmTone(4)
        val songs =
            recordings.mapIndexed { index, recording ->
                SearchSong(
                    recording,
                    "Queued fixture ${index + 1}",
                    "Queue artist",
                    "",
                    4000uL,
                    emptyList(),
                )
            }
        val server = ServerSocket(0, 16, InetAddress.getByName("127.0.0.1"))
        val workers = Executors.newFixedThreadPool(4)
        workers.submit {
            while (!server.isClosed) {
                try {
                    val socket = server.accept()
                    workers.submit {
                        socket.use {
                            it.soTimeout = 5000
                            val reader = it.getInputStream().bufferedReader(Charsets.US_ASCII)
                            val request = reader.readLine().orEmpty()
                            while (!reader.readLine().isNullOrEmpty()) {}
                            val search = request.contains("/search3?")
                            val listing =
                                songs.mapIndexed { index, song ->
                                    """{"id":"fixture${index + 1}","title":"${song.title}","artist":"${song.artist}","suffix":"wav","duration":4,"musicBrainzId":"${song.recording}"}"""
                                }
                            val body =
                                if (search)
                                    """{"subsonic-response":{"status":"ok","version":"1.16.1","searchResult3":{"song":[${listing.joinToString(",")}]}}}"""
                                        .toByteArray()
                                else audio
                            if (!search) Thread.sleep(700)
                            it.getOutputStream().apply {
                                write(
                                    "HTTP/1.1 200 OK\r\nContent-Type: ${if (search) "application/json" else "audio/wav"}\r\nContent-Length: ${body.size}\r\nConnection: close\r\n\r\n"
                                        .toByteArray(Charsets.US_ASCII)
                                )
                                write(body)
                                flush()
                            }
                        }
                    }
                } catch (dropped: java.net.SocketException) {
                    if (server.isClosed) break
                }
            }
        }
        val made = mutableListOf<Uri>()
        try {
            ActivityScenario.launch(MainActivity::class.java).use {
                val library = (context.applicationContext as ResonateApplication).library
                val features = library.features
                features.providerAccounts.value =
                    ProviderAccountBook()
                        .saved(
                            1L,
                            DownloadAccount(
                                MusicProvider.SUBSONIC,
                                "http://127.0.0.1:${server.localPort}",
                                "fixture",
                                "fixture",
                                "",
                                "",
                                "",
                            ),
                        )
                val folder = runBlocking { features.useDownloads() }
                features.download(songs, folder)
                var most = 0
                val deadline = System.nanoTime() + 60_000_000_000L
                while (true) {
                    val jobs = features.downloads.jobs.value
                    most = maxOf(most, jobs.count { it.state.inFlight })
                    if (jobs.size == 3 && jobs.all { it.state.settled }) break
                    if (System.nanoTime() > deadline) throw AssertionError("The queue did not settle: $jobs")
                    Thread.sleep(25)
                }
                val jobs = features.downloads.jobs.value
                assertEquals(listOf(DownloadState.DONE), jobs.map { it.state }.distinct())
                assertTrue("at most two run at once, saw $most", most in 1..DownloadPlan.AT_ONCE)
                assertEquals("Downloaded 3 of 3 songs", features.downloadStatus.value)
                runBlocking { library.refresh() }
                val landed = library.allTracks.value.filter { it.title.startsWith("Queued fixture") }
                assertEquals(3, landed.size)
                landed.forEach { made.add(Uri.parse(it.uri)) }
            }
        } finally {
            made.forEach { runCatching { MusicDocuments.delete(context, it) } }
            server.close()
            workers.shutdownNow()
            workers.awaitTermination(5, TimeUnit.SECONDS)
        }
    }
}
