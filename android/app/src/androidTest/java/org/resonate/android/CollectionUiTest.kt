package org.resonate.android

import android.content.ComponentName
import android.content.ContentValues
import android.provider.MediaStore
import androidx.media3.session.MediaController
import androidx.media3.session.SessionToken
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.uiautomator.By
import androidx.test.uiautomator.BySelector
import androidx.test.uiautomator.Direction
import androidx.test.uiautomator.UiDevice
import androidx.test.uiautomator.UiObject2
import java.io.ByteArrayOutputStream
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.util.concurrent.TimeUnit
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.resonate.android.nativecore.Track
import org.resonate.android.nativecore.Transport

@androidx.annotation.OptIn(markerClass = [androidx.media3.common.util.UnstableApi::class])
@RunWith(AndroidJUnit4::class)
class CollectionUiTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val device = UiDevice.getInstance(instrumentation)

    @Test
    fun flutterFavouritesSurviveActivityRecreation() =
        withLibrary { library, controller, scenario, tracks ->
            play(controller, tracks, 0)
            await(By.descContains("Song progress")).click()
            await(By.desc("Add to favourites")).click()
            eventually { library.allTracks.value.any { it.id == tracks[0].id && it.favourite } }
            await(By.desc("Remove from favourites"))
            scenario.recreate()
            await(By.desc("Remove from favourites"))
            assertTrue(library.allTracks.value.first { it.id == tracks[0].id }.favourite)
        }

    @Test
    fun flutterPlaylistPlaysTheTappedRepeatedRowAndReflectsUndo() =
        withLibrary { library, controller, _, tracks ->
            val created = CompletableDeferred<ULong>()
            library.request { native ->
                val id = native.createPlaylist("Flutter test mix")
                native.addToPlaylist(id, listOf(tracks[0].id, tracks[0].id, tracks[1].id))
                created.complete(id)
            }
            val id = runBlocking { withTimeout(10000) { created.await() } }
            library.features.loadPlaylists()
            try {
                scrollTo(By.descContains("Flutter test mix")).click()
                scrollTo(By.descContains("Play song 2:")).click()
                eventually {
                    library.snapshot.value?.queueIndex == 1u &&
                        library.snapshot.value?.transport == Transport.PLAYING
                }
                instrumentation.runOnMainSync { controller.pause() }
                scrollTo(By.descContains("Options for song 2")).click()
                await(By.descContains("Move down")).click()
                eventually {
                    library.features.playlistRows.value.map { it.id } ==
                        listOf(tracks[0].id, tracks[1].id, tracks[0].id)
                }
                scrollTo(By.desc("Playlist options"), Direction.UP).click()
                await(By.descContains("Undo last edit")).click()
                eventually {
                    library.features.playlistRows.value.map { it.id } ==
                        listOf(tracks[0].id, tracks[0].id, tracks[1].id)
                }
                assertEquals(3, library.features.playlistRows.value.size)
            } finally {
                val deleted = CompletableDeferred<Unit>()
                library.request { native ->
                    native
                        .playlists()
                        .find { it.id == id }
                        ?.let { native.deletePlaylist(it.id, it.revision) }
                    deleted.complete(Unit)
                }
                runBlocking { withTimeout(10000) { deleted.await() } }
            }
        }

    private fun withLibrary(
        action:
            (
                LibraryRepository,
                MediaController,
                ActivityScenario<MainActivity>,
                List<Track>,
            ) -> Unit
    ) {
        val context = instrumentation.targetContext
        val resolver = context.contentResolver
        val artist = "Collection artist & guest ${System.nanoTime()}"
        val uris =
            (1..2).map { number ->
                val values =
                    ContentValues().apply {
                        put(MediaStore.Audio.Media.DISPLAY_NAME, "Collection fixture $number.wav")
                        put(MediaStore.Audio.Media.MIME_TYPE, "audio/wav")
                        put(MediaStore.Audio.Media.RELATIVE_PATH, "Music/ResonateTests")
                    }
                requireNotNull(resolver.insert(MediaStore.Audio.Media.EXTERNAL_CONTENT_URI, values))
                    .also { uri ->
                        requireNotNull(resolver.openOutputStream(uri)).use {
                            it.write(taggedTone("Collection fixture $number", artist))
                        }
                    }
            }
        try {
            ActivityScenario.launch(MainActivity::class.java).use { scenario ->
                val library = (context.applicationContext as ResonateApplication).library
                val controller =
                    MediaController.Builder(
                            context,
                            SessionToken(
                                context,
                                ComponentName(context, PlaybackService::class.java),
                            ),
                        )
                        .buildAsync()
                        .get(15, TimeUnit.SECONDS)
                try {
                    val indexed = CompletableDeferred<List<Track>>()
                    library.request { native ->
                        indexed.complete(
                            uris.mapIndexed { index, uri ->
                                native.indexDocument(
                                    uri.toString(),
                                    "Collection fixture ${index + 1}.wav",
                                )
                            }
                        )
                    }
                    val tracks = runBlocking {
                        withTimeout(10000) { indexed.await().also { library.refresh() } }
                    }
                    await(By.desc("Your library"))
                    action(library, controller, scenario, tracks)
                } finally {
                    library.allTracks.value
                        .filter { it.artist == artist && it.favourite }
                        .forEach { library.setFavourite(it.id, false) }
                    instrumentation.runOnMainSync {
                        controller.stop()
                        controller.release()
                    }
                    library.search("")
                }
            }
        } finally {
            uris.forEach { resolver.delete(it, null, null) }
        }
    }

    private fun play(controller: MediaController, tracks: List<Track>, index: Int) {
        instrumentation.runOnMainSync {
            controller.setMediaItems(tracks.map { it.mediaItem() }, index, 0)
            controller.prepare()
            controller.play()
        }
        val library =
            (instrumentation.targetContext.applicationContext as ResonateApplication).library
        eventually { library.snapshot.value?.transport == Transport.PLAYING }
        instrumentation.runOnMainSync { controller.pause() }
        eventually { library.snapshot.value?.transport == Transport.PAUSED }
    }

    private fun taggedTone(title: String, artist: String): ByteArray {
        val info = ByteArrayOutputStream()
        info.write("INFO".toByteArray())
        listOf("INAM" to title, "IART" to artist, "IPRD" to "Collection album: / & ? #").forEach {
            (tag, value) ->
            val bytes = (value + '\u0000').toByteArray()
            info.write(tag.toByteArray())
            info.write(
                ByteBuffer.allocate(4).order(ByteOrder.LITTLE_ENDIAN).putInt(bytes.size).array()
            )
            info.write(bytes)
            if (bytes.size % 2 == 1) info.write(0)
        }
        val base = pcmTone(60)
        val result =
            base +
                "LIST".toByteArray() +
                ByteBuffer.allocate(4).order(ByteOrder.LITTLE_ENDIAN).putInt(info.size()).array() +
                info.toByteArray()
        ByteBuffer.wrap(result).order(ByteOrder.LITTLE_ENDIAN).putInt(4, result.size - 8)
        return result
    }

    private fun await(selector: BySelector): UiObject2 {
        var result: UiObject2? = null
        eventually {
            result = device.findObject(selector)
            result != null
        }
        return requireNotNull(result)
    }

    private fun scrollTo(selector: BySelector, direction: Direction = Direction.DOWN): UiObject2 {
        repeat(14) {
            instrumentation.uiAutomation.clearCache()
            device.findObject(selector)?.let {
                return it
            }
            device.findObject(By.scrollable(true))?.scroll(direction, 0.7f)
            device.waitForIdle()
        }
        return await(selector)
    }

    private fun eventually(condition: () -> Boolean) {
        val deadline = System.nanoTime() + 15_000_000_000L
        while (true) {
            instrumentation.uiAutomation.clearCache()
            if (condition()) return
            if (System.nanoTime() > deadline) {
                val hierarchy =
                    ByteArrayOutputStream().use {
                        device.dumpWindowHierarchy(it)
                        it.toString()
                    }
                throw AssertionError("Collection state did not settle: $hierarchy")
            }
            Thread.sleep(100)
        }
    }
}
