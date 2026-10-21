package org.resonate.android

import android.content.ComponentName
import android.content.ContentValues
import android.provider.MediaStore
import androidx.media3.session.LibraryResult
import androidx.media3.session.MediaBrowser
import androidx.media3.session.SessionToken
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import java.util.concurrent.TimeUnit
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.resonate.android.nativecore.Track

@androidx.annotation.OptIn(markerClass = [androidx.media3.common.util.UnstableApi::class])
@RunWith(AndroidJUnit4::class)
class LibraryBrowseTest {
    @Test
    fun aCarBrowsesTheFavouritesAndPlaysOneWithItsNeighbours() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val context = instrumentation.targetContext
        val resolver = context.contentResolver
        val uris =
            listOf("Browse one.wav", "Browse two.wav").map { name ->
                val values =
                    ContentValues().apply {
                        put(MediaStore.Audio.Media.DISPLAY_NAME, name)
                        put(MediaStore.Audio.Media.MIME_TYPE, "audio/wav")
                        put(MediaStore.Audio.Media.RELATIVE_PATH, "Music/ResonateTests")
                    }
                val uri =
                    requireNotNull(
                        resolver.insert(MediaStore.Audio.Media.EXTERNAL_CONTENT_URI, values)
                    )
                requireNotNull(resolver.openOutputStream(uri)).use { it.write(pcmTone(5)) }
                name to uri
            }
        try {
            ActivityScenario.launch(MainActivity::class.java).use {
                val library = (context.applicationContext as ResonateApplication).library
                val indexed = CompletableDeferred<List<Track>>()
                library.request { native ->
                    indexed.complete(
                        uris.map { (name, uri) ->
                            native.indexDocument(uri.toString(), name).also {
                                native.favourite(it.id, true)
                            }
                        }
                    )
                }
                val tracks = runBlocking { withTimeout(15000) { indexed.await() } }
                val browser =
                    MediaBrowser.Builder(
                            context,
                            SessionToken(
                                context,
                                ComponentName(context, PlaybackService::class.java),
                            ),
                        )
                        .buildAsync()
                        .get(15, TimeUnit.SECONDS)
                try {
                    val root = asked { browser.getLibraryRoot(null) }
                    assertEquals(LibraryResult.RESULT_SUCCESS, root.resultCode)
                    val folders =
                        asked { browser.getChildren(root.value!!.mediaId, 0, 50, null) }.value!!
                    assertEquals(
                        listOf("Albums", "Artists", "Playlists", "Favourites"),
                        folders.map { it.mediaMetadata.title.toString() },
                    )
                    val favourites =
                        asked { browser.getChildren(BrowseIds.FAVOURITES, 0, 50, null) }.value!!
                    assertTrue(tracks.all { track ->
                        favourites.any { BrowseIds.songOf(it.mediaId)?.first == track.id }
                    })
                    val chosen = favourites.first { BrowseIds.songOf(it.mediaId)?.first == tracks[1].id }
                    instrumentation.runOnMainSync {
                        browser.setMediaItem(chosen)
                        browser.prepare()
                        browser.play()
                    }
                    eventually { library.snapshot.value?.current?.id == tracks[1].id }
                    assertTrue(library.queue.value.size >= 2)
                    val found = asked { browser.search("Browse one", null) }
                    assertEquals(LibraryResult.RESULT_SUCCESS, found.resultCode)
                } finally {
                    instrumentation.runOnMainSync { browser.release() }
                    library.request { native -> tracks.forEach { native.favourite(it.id, false) } }
                }
            }
        } finally {
            uris.forEach { (_, uri) -> resolver.delete(uri, null, null) }
        }
    }

    private fun <T> asked(call: () -> com.google.common.util.concurrent.ListenableFuture<T>): T {
        var pending: com.google.common.util.concurrent.ListenableFuture<T>? = null
        InstrumentationRegistry.getInstrumentation().runOnMainSync { pending = call() }
        return pending!!.get(15, TimeUnit.SECONDS)
    }

    private fun eventually(condition: () -> Boolean) {
        val deadline = System.nanoTime() + 15_000_000_000L
        while (!condition()) {
            if (System.nanoTime() > deadline) throw AssertionError("Browsing did not settle")
            Thread.sleep(50)
        }
    }
}
