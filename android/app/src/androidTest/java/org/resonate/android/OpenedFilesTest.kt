package org.resonate.android

import android.content.ContentValues
import android.net.Uri
import android.provider.MediaStore
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import java.io.File
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class OpenedFilesTest {
    private val context = InstrumentationRegistry.getInstrumentation().targetContext
    private val resolver = context.contentResolver
    private val folder = File(context.filesDir, OpenedNames.FOLDER)

    private fun made(name: String, bytes: ByteArray): Uri {
        val values =
            ContentValues().apply {
                put(MediaStore.Audio.Media.DISPLAY_NAME, name)
                put(MediaStore.Audio.Media.MIME_TYPE, "audio/wav")
                put(MediaStore.Audio.Media.RELATIVE_PATH, "Music/ResonateTests")
            }
        val uri = requireNotNull(resolver.insert(MediaStore.Audio.Media.EXTERNAL_CONTENT_URI, values))
        requireNotNull(resolver.openOutputStream(uri)).use { it.write(bytes) }
        return uri
    }

    private fun kept(): Set<String> = folder.list().orEmpty().toSet()

    @Test
    fun aFileOpenedTwiceIsKeptOnce() {
        val uri = made("Opened twice.wav", pcmTone(2))
        try {
            ActivityScenario.launch(MainActivity::class.java).use {
                val library = (context.applicationContext as ResonateApplication).library
                val before = kept()

                val first = runBlocking { library.openedFiles.track(uri) }
                val second = runBlocking { library.openedFiles.track(uri) }
                val added = kept() - before

                assertEquals(first.uri, second.uri)
                assertEquals(first.id, second.id)
                assertTrue(added.size <= 1)
                assertFalse(added.any { it.contains(" (2)") })
            }
        } finally {
            runCatching { resolver.delete(uri, null, null) }
        }
    }

    @Test
    fun aFileThatDoesNotIndexLeavesNoCopy() {
        val uri = made("Opened damaged.wav", ByteArray(4096) { 0x5A })
        try {
            ActivityScenario.launch(MainActivity::class.java).use {
                val library = (context.applicationContext as ResonateApplication).library
                val before = kept()

                val refused = runCatching { runBlocking { library.openedFiles.track(uri) } }

                assertTrue(refused.isFailure)
                assertEquals(before, kept())
            }
        } finally {
            runCatching { resolver.delete(uri, null, null) }
        }
    }

    @Test
    fun aFileUriOrTheAppsOwnProviderIsNeverOpened() {
        ActivityScenario.launch(MainActivity::class.java).use {
            val library = (context.applicationContext as ResonateApplication).library
            val ownFile = File(context.filesDir, "private.wav").apply { writeBytes(pcmTone(1)) }
            try {
                val before = kept()
                val viaFile = Uri.fromFile(ownFile)
                val viaOwnProvider =
                    Uri.parse("content://${context.packageName}.files/shared/private.wav")
                val viaOwnProviderForAUser =
                    Uri.parse("content://0@${context.packageName}.files/shared/private.wav")

                assertFalse(library.openedFiles.mayOpen(viaFile))
                assertFalse(library.openedFiles.mayOpen(viaOwnProvider))
                assertFalse(library.openedFiles.mayOpen(viaOwnProviderForAUser))
                assertTrue(runCatching { runBlocking { library.openedFiles.track(viaFile) } }.isFailure)
                assertEquals(before, kept())
            } finally {
                ownFile.delete()
            }
        }
    }
}
