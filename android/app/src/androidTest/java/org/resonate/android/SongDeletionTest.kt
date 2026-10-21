package org.resonate.android

import android.content.ContentValues
import android.net.Uri
import android.provider.MediaStore
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.resonate.android.nativecore.Track

@RunWith(AndroidJUnit4::class)
class SongDeletionTest {
    private val context = InstrumentationRegistry.getInstrumentation().targetContext
    private val resolver = context.contentResolver

    private fun made(name: String): Uri {
        val values =
            ContentValues().apply {
                put(MediaStore.Audio.Media.DISPLAY_NAME, name)
                put(MediaStore.Audio.Media.MIME_TYPE, "audio/wav")
                put(MediaStore.Audio.Media.RELATIVE_PATH, "Music/ResonateTests")
            }
        val uri = requireNotNull(resolver.insert(MediaStore.Audio.Media.EXTERNAL_CONTENT_URI, values))
        requireNotNull(resolver.openOutputStream(uri)).use { it.write(pcmTone(3)) }
        return uri
    }

    private fun stands(uri: Uri): Boolean =
        try {
            resolver.query(uri, arrayOf(MediaStore.MediaColumns._ID), null, null, null)?.use {
                it.moveToFirst()
            } ?: false
        } catch (gone: SecurityException) {
            false
        }

    private fun indexed(library: LibraryRepository, uri: Uri, name: String): Track =
        runBlocking { library.native().indexDocument(uri.toString(), name) }

    @Test
    fun aDeletedSongLeavesItsFileAndTheCatalog() {
        val uri = made("Deletion one.wav")
        try {
            ActivityScenario.launch(MainActivity::class.java).use {
                val library = (context.applicationContext as ResonateApplication).library
                val track = indexed(library, uri, "Deletion one.wav")

                val deleted =
                    runBlocking { SongDeletion(context, library) { false }.delete(listOf(track)) }
                runBlocking { library.refresh() }

                assertEquals(1, deleted.removed)
                assertEquals(0, deleted.kept)
                assertTrue(!stands(uri))
                assertNull(library.findTrack(track.id))
            }
        } finally {
            runCatching { resolver.delete(uri, null, null) }
        }
    }

    @Test
    fun identicalFilesFoldIntoTheCopyKept() {
        val first = made("Identical a.wav")
        val second = made("Identical b.wav")
        try {
            ActivityScenario.launch(MainActivity::class.java).use {
                val library = (context.applicationContext as ResonateApplication).library
                val a = indexed(library, first, "Identical a.wav")
                val b = indexed(library, second, "Identical b.wav")
                runBlocking { library.native().favourite(a.id, true) }

                val sets =
                    runBlocking { library.native().identicalSongs() }.filter { set ->
                        (listOf(set.kept) + set.copies).any { it.id == a.id || it.id == b.id }
                    }
                assertEquals(1, sets.size)
                val set = sets.single()
                val folded = runBlocking { SongDeletion(context, library) { false }.foldCopies(listOf(set)) }
                runBlocking { library.refresh() }

                assertEquals(1, folded.folded)
                assertEquals(0, folded.kept)
                assertEquals(1, listOf(first, second).count { stands(it) })
                assertEquals(1, listOf(a.id, b.id).count { library.findTrack(it) != null })
                assertTrue(library.findTrack(set.kept.id)?.favourite == true)
            }
        } finally {
            runCatching { resolver.delete(first, null, null) }
            runCatching { resolver.delete(second, null, null) }
        }
    }
}
