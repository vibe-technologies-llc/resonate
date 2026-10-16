package org.resonate.android

import android.Manifest
import android.content.ContentUris
import android.net.Uri
import android.os.Environment
import android.os.ParcelFileDescriptor
import android.provider.MediaStore
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import java.io.File
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.resonate.android.nativecore.Documents
import org.resonate.android.nativecore.MobilePlayer

@RunWith(AndroidJUnit4::class)
class DownloadsFolderTest {
    @Test
    fun downloadsArePendingUntilValidatedAndRescanKeepsIdentity() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val resolver = context.contentResolver
        val folder =
            File(context.cacheDir, "downloads-test-${System.nanoTime()}").apply { mkdirs() }
        val name = "resonate-fixture-${System.nanoTime()}.wav"
        val uri = MusicDocuments.create(context, MusicDocuments.downloads, "audio/wav", name)
        var duplicate: Uri? = null
        val player =
            MobilePlayer(
                folder.absolutePath,
                object : Documents {
                    override fun openDocument(uri: String): Int =
                        requireNotNull(resolver.openFileDescriptor(Uri.parse(uri), "r")).detachFd()
                },
            )
        try {
            val audio = pcmTone(1)
            resolver.openOutputStream(uri, "w")!!.use { it.write(audio) }
            assertFalse(MusicDocuments.downloadedAudio(context).any { it.uri == uri })
            resolver
                .query(uri, arrayOf(MediaStore.MediaColumns.RELATIVE_PATH), null, null, null)!!
                .use {
                    assertTrue(it.moveToFirst())
                    assertEquals("${Environment.DIRECTORY_DOWNLOADS}/", it.getString(0))
                }
            val indexed = player.indexDocument(uri.toString(), name)
            assertEquals(44100u, indexed.rate)
            MusicDocuments.publish(context, uri)
            val found = MusicDocuments.downloadedAudio(context).first { it.uri == uri }
            assertEquals(name, found.name)
            player.favourite(indexed.id, true)
            val rescanned = player.indexDocument(found.uri.toString(), found.name)
            assertEquals(indexed.id, rescanned.id)
            assertTrue(rescanned.favourite)
            assertArrayEquals(audio, resolver.openInputStream(uri)!!.use { it.readBytes() })
            duplicate = MusicDocuments.create(context, MusicDocuments.downloads, "audio/wav", name)
            assertNotEquals(uri, duplicate)
            assertArrayEquals(audio, resolver.openInputStream(uri)!!.use { it.readBytes() })
        } finally {
            duplicate?.let { MusicDocuments.delete(context, it) }
            MusicDocuments.delete(context, uri)
            player.shutdown()
            player.close()
            folder.deleteRecursively()
        }
    }

    @Test
    fun musicPermissionReadsAudioCreatedByAnotherAppInDownloads() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val context = instrumentation.targetContext
        val automation = instrumentation.uiAutomation
        val name = "resonate-other-fixture-${System.nanoTime()}.wav"
        val collection = MusicDocuments.downloads
        shell(
            "content insert --uri $collection --bind _display_name:s:$name --bind mime_type:s:audio/wav --bind relative_path:s:Download/ --bind is_pending:i:1"
        )
        val foundRow =
            shell("content query --uri $collection?includePending=1 --projection _id:_display_name")
                .lineSequence()
                .first { it.contains(name) }
        val id =
            requireNotNull(Regex("_id=(\\d+)").find(foundRow)) { foundRow }.groupValues[1].toLong()
        val uri = ContentUris.withAppendedId(collection, id)
        assertEquals(MediaStore.AUTHORITY, uri.authority)
        val canonical =
            ContentUris.withAppendedId(
                MediaStore.Files.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY),
                ContentUris.parseId(uri),
            )
        try {
            val pipes = automation.executeShellCommandRw("content write --uri $uri")
            ParcelFileDescriptor.AutoCloseOutputStream(pipes[1]).use { it.write(pcmTone(1)) }
            ParcelFileDescriptor.AutoCloseInputStream(pipes[0]).use { it.readBytes() }
            shell("content update --uri $uri --bind is_pending:i:0")
            automation.grantRuntimePermission(
                context.packageName,
                Manifest.permission.READ_MEDIA_AUDIO,
            )
            val songs = MusicDocuments.downloadedAudio(context)
            assertTrue("Expected $canonical in $songs", songs.any { it.uri == canonical })
            val found = songs.first { it.uri == canonical }
            assertEquals(name, found.name)
            assertArrayEquals(
                pcmTone(1),
                context.contentResolver.openInputStream(found.uri)!!.use { it.readBytes() },
            )
        } finally {
            shell("content delete --uri $uri")
        }
    }

    private fun shell(command: String): String =
        ParcelFileDescriptor.AutoCloseInputStream(
                InstrumentationRegistry.getInstrumentation()
                    .uiAutomation
                    .executeShellCommand(command)
            )
            .bufferedReader()
            .use { it.readText() }
}
