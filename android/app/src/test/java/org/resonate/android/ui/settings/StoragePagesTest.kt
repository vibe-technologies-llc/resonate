package org.resonate.android.ui.settings

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.resonate.android.English
import org.resonate.android.nativecore.SizedSong
import org.resonate.android.nativecore.SizedSongAfter
import org.resonate.android.nativecore.Track

class StoragePagesTest {
    private fun sized(id: Long, bytes: Long) =
        SizedSong(
            Track(
                id = id.toULong(),
                uri = "content://test/$id",
                name = "$id.flac",
                title = "Song $id",
                artist = "Artist",
                album = "Album",
                durationMs = 60_000uL,
                rate = 44_100u,
                codec = "FLAC",
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
            ),
            bytes.toULong(),
        )

    @Test
    fun theNextPageIsAskedFromTheLastSongShown() {
        assertNull(SizedPages.after(emptyList()))
        assertEquals(
            SizedSongAfter(300uL, 9uL),
            SizedPages.after(listOf(sized(4, 900), sized(9, 300))),
        )
    }

    @Test
    fun aPageShorterThanAskedEndsTheListAndASongIsShownOnce() {
        val first = SizedPages.reread(listOf(sized(1, 900), sized(2, 800)), asked = 2)

        assertFalse(first.ended)
        assertFalse(first.reading)

        val next = SizedPages.appended(first.copy(reading = true), listOf(sized(2, 800), sized(3, 700)), asked = 2)

        assertEquals(listOf(1uL, 2uL, 3uL), next.songs.map { it.track.id })
        assertFalse(next.ended)

        val last = SizedPages.appended(next, listOf(sized(4, 100)), asked = 2)

        assertTrue(last.ended)
        assertEquals(4, last.songs.size)
    }

    @Test
    fun aRereadAsksForEverySongAlreadyShownAndAtLeastAPage() {
        assertEquals(SizedPages.PAGE, SizedPages.rereadAsking(StorageView()))
        assertEquals(
            SizedPages.PAGE + 1,
            SizedPages.rereadAsking(StorageView((0..SizedPages.PAGE.toLong()).map { sized(it, 1_000 - it) })),
        )
    }

    @Test
    fun aFailedReadKeepsTheSongsShownAndSaysWhy() {
        val shown = SizedPages.reread(listOf(sized(1, 900)), asked = 2).copy(reading = true)

        val failed = SizedPages.failed(shown, "The songs could not be listed")

        assertEquals(shown.songs, failed.songs)
        assertFalse(failed.reading)
        assertEquals(English.string(org.resonate.android.R.string.storage_failed), failed.failed)
    }

    @Test
    fun storageIsFoundBySpaceSizeAndCompression() {
        val found = { query: String -> matchingSettings(English, query).map { it.setting } }

        assertEquals(listOf(Setting.STORAGE), found("storage"))
        assertTrue(Setting.STORAGE in found("space"))
        assertTrue(Setting.STORAGE in found("largest"))
        assertTrue(Setting.STORAGE in found("compress"))
        assertTrue(Setting.STORAGE in found("size"))
    }
}
