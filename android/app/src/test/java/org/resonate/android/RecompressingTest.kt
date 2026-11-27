package org.resonate.android

import java.io.ByteArrayInputStream
import java.io.File
import java.io.FileNotFoundException
import java.io.IOException
import java.io.InputStream
import java.io.OutputStream
import kotlin.io.path.createTempDirectory
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.resonate.android.nativecore.RecompressCeiling
import org.resonate.android.nativecore.Track

class RecompressingTest {
    private class Documents(
        val held: MutableMap<String, ByteArray>,
        var failingAfter: Int? = null,
        var failingTimes: Int = 0,
    ) : WrittenDocuments {
        override fun open(uri: String): InputStream =
            ByteArrayInputStream(held[uri] ?: throw FileNotFoundException(uri))

        override fun replace(uri: String): OutputStream {
            held[uri] = ByteArray(0)
            val failing = if (failingTimes > 0) failingAfter.also { failingTimes-- } else null
            return object : OutputStream() {
                private var written = 0

                override fun write(byte: Int) {
                    if (failing != null && written >= failing) throw IOException("cut short")
                    held[uri] = held.getValue(uri) + byte.toByte()
                    written++
                }
            }
        }

        override fun modifiedMs(uri: String): Long = MODIFIED
    }

    private fun size(bytes: Long): String = "$bytes B"

    private fun song(codec: String) =
        Track(
            id = 7uL,
            uri = URI,
            name = "7.flac",
            title = "Song",
            artist = "Artist",
            album = "Album",
            durationMs = 60_000uL,
            rate = 44_100u,
            codec = codec,
            favourite = false,
            albumArtist = "",
            genre = "",
            date = "",
            trackNumber = null,
            discNumber = null,
            bits = 16u,
            channels = 2u,
            recording = null,
            release = null,
            artistMbid = null,
            artRevision = 0uL,
            queueId = null,
        )

    private fun stage(folder: File, bytes: ByteArray): File = File(folder, "stage.flac").apply { writeBytes(bytes) }

    @Test
    fun theCeilingIsLosslessUntilChosenAndReadBackByTheNameKept() {
        assertEquals(RecompressTo.LOSSLESS, RecompressTo.read { null })
        assertEquals(RecompressTo.LOSSLESS, RecompressTo.read { "nonsense" })
        assertEquals(RecompressTo.CD, RecompressTo.read(mapOf(RecompressTo.KEY to "cd")::get))
        assertEquals("recompress-to", RecompressTo.KEY)
        assertEquals(
            listOf(RecompressCeiling.KEPT, RecompressCeiling.STUDIO, RecompressCeiling.HIGH, RecompressCeiling.CD),
            RecompressTo.entries.map { it.ceiling },
        )
        RecompressTo.entries.forEach { chosen ->
            assertEquals(chosen, RecompressTo.read(mapOf(RecompressTo.KEY to chosen.stored)::get))
        }
    }

    @Test
    fun onlyAFlacIsOfferedForReCompression() {
        assertTrue(Recompressible.isFlac(song(codec = "FLAC")))
        assertTrue(Recompressible.isFlac(song(codec = "flac")))
        assertFalse(Recompressible.isFlac(song(codec = "MP3")))
    }

    @Test
    fun oneSongSaysWhatItBecameOrWhyItWasKept() {
        val shrunk = listOf(RecompressTold("Song", RecompressOutcome.Shrunk(1_000uL, 600uL)))
        val kept = listOf(RecompressTold("Song", RecompressOutcome.Kept("Kept as it was, since it holds no sound")))

        assertEquals(
            RecompressNotice("Song is now 600 B, 400 B smaller", failure = false),
            RecompressSentences.told(English, shrunk, 1, ::size),
        )
        assertEquals(
            RecompressNotice("Song: Kept as it was, since it holds no sound", failure = false),
            RecompressSentences.told(English, kept, 1, ::size),
        )
        assertEquals(
            RecompressNotice("Song could not be written over, so the original was put back", failure = true),
            RecompressSentences.told(English, listOf(RecompressTold("Song", RecompressOutcome.PutBack)), 1, ::size),
        )
    }

    @Test
    fun manySongsAreCountedWithTheBytesSaved() {
        val told =
            listOf(
                RecompressTold("One", RecompressOutcome.Shrunk(1_000uL, 600uL)),
                RecompressTold("Two", RecompressOutcome.Shrunk(500uL, 400uL)),
                RecompressTold("Three", RecompressOutcome.Kept("Kept as it was")),
                RecompressTold("Four", RecompressOutcome.NotAllowed),
            )

        assertEquals(
            RecompressNotice(
                "2 files re-compressed, 500 B saved; 1 kept as it was; 1 was not allowed to be written",
                failure = true,
            ),
            RecompressSentences.told(English, told, 4, ::size),
        )
        assertEquals(500L, RecompressSentences.saved(told))
    }

    @Test
    fun aRunStoppedSaysHowFarItCame() {
        val told =
            listOf(
                RecompressTold("One", RecompressOutcome.Shrunk(1_000uL, 600uL)),
                RecompressTold("Two", RecompressOutcome.Stopped),
            )

        assertEquals(
            "Stopped after 1 of 5: 1 file re-compressed, 400 B saved",
            RecompressSentences.told(English, told, 5, ::size)?.text,
        )
        assertEquals(
            "Stopped before any file was re-compressed",
            RecompressSentences.told(English, emptyList(), 5, ::size)?.text,
        )
        assertEquals("No FLAC files to re-compress", RecompressSentences.told(English, emptyList(), 0, ::size)?.text)
    }

    @Test
    fun aStageWrittenWhollyTakesThePlaceOfTheOriginalAndLeavesNothingAside() {
        val folder = createTempDirectory().toFile()
        val documents = Documents(mutableMapOf(URI to ORIGINAL))
        val writing = StageWriting(documents, File(folder, StageWriting.ASIDE_FOLDER))

        val written = writing.over(URI, stage(folder, SMALLER), 7uL)

        assertTrue(written is StageWritten.Written)
        assertEquals(MODIFIED, (written as StageWritten.Written).modifiedMs)
        assertArrayEquals(SMALLER, documents.held[URI])
        assertArrayEquals(ORIGINAL, written.aside.readBytes())

        writing.settled(written.aside)

        assertTrue(File(folder, StageWriting.ASIDE_FOLDER).listFiles().orEmpty().isEmpty())
    }

    @Test
    fun aWriteCutShortPutsTheOriginalBack() {
        val folder = createTempDirectory().toFile()
        val documents = Documents(mutableMapOf(URI to ORIGINAL), failingAfter = 2, failingTimes = 1)
        val writing = StageWriting(documents, File(folder, StageWriting.ASIDE_FOLDER))

        val written = writing.over(URI, stage(folder, SMALLER), 7uL)

        assertTrue(written is StageWritten.PutBack)
        assertArrayEquals(ORIGINAL, documents.held[URI])
        assertTrue(File(folder, StageWriting.ASIDE_FOLDER).listFiles().orEmpty().isEmpty())
    }

    @Test
    fun anOriginalThatCannotBePutBackIsKeptAside() {
        val folder = createTempDirectory().toFile()
        val documents = Documents(mutableMapOf(URI to ORIGINAL), failingAfter = 2, failingTimes = 2)
        val writing = StageWriting(documents, File(folder, StageWriting.ASIDE_FOLDER))

        val written = writing.over(URI, stage(folder, SMALLER), 7uL)

        assertTrue(written is StageWritten.KeptAside)
        assertArrayEquals(ORIGINAL, (written as StageWritten.KeptAside).aside.readBytes())
    }

    @Test
    fun anOriginalThatCannotBeReadIsNeverWrittenOver() {
        val folder = createTempDirectory().toFile()
        val documents = Documents(mutableMapOf())
        val writing = StageWriting(documents, File(folder, StageWriting.ASIDE_FOLDER))

        val written = writing.over(URI, stage(folder, SMALLER), 7uL)

        assertTrue(written is StageWritten.Untouched)
        assertNull(documents.held[URI])
        assertTrue(File(folder, StageWriting.ASIDE_FOLDER).listFiles().orEmpty().isEmpty())
    }

    @Test
    fun aDocumentWrittenButRefusedAfterwardsIsPutBackFromAside() {
        val folder = createTempDirectory().toFile()
        val documents = Documents(mutableMapOf(URI to ORIGINAL))
        val writing = StageWriting(documents, File(folder, StageWriting.ASIDE_FOLDER))
        val written = writing.over(URI, stage(folder, SMALLER), 7uL) as StageWritten.Written

        assertTrue(writing.putBack(URI, written.aside))

        assertArrayEquals(ORIGINAL, documents.held[URI])
        assertFalse(written.aside.exists())
    }

    private companion object {
        const val URI = "content://test/document/7"
        const val MODIFIED = 1_700_000_000_000L
        val ORIGINAL = ByteArray(64) { it.toByte() }
        val SMALLER = ByteArray(16) { (it * 3).toByte() }
    }
}
