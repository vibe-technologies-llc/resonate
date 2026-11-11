package org.resonate.android

import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import java.io.File
import java.nio.file.Files
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class OpenedSourcesTest {
    private val own = setOf("org.resonate.android.art", "org.resonate.android.files")

    @Test
    fun anotherAppsContentIsOpened() {
        assertTrue(OpenedSources.mayOpen("content", "com.android.providers.downloads.documents", own))
        assertTrue(OpenedSources.mayOpen("content", "media", own))
    }

    @Test
    fun everySchemeButContentIsRefused() {
        assertFalse(OpenedSources.mayOpen("file", "", own))
        assertFalse(OpenedSources.mayOpen("file", null, own))
        assertFalse(OpenedSources.mayOpen("android.resource", "org.stranger.app", own))
        assertFalse(OpenedSources.mayOpen("https", "example.org", own))
        assertFalse(OpenedSources.mayOpen(null, "media", own))
        assertFalse(OpenedSources.mayOpen("content", null, own))
        assertFalse(OpenedSources.mayOpen("content", "", own))
    }

    @Test
    fun theAppsOwnAuthoritiesAreRefusedHoweverSpelled() {
        assertFalse(OpenedSources.mayOpen("content", "org.resonate.android.files", own))
        assertFalse(OpenedSources.mayOpen("content", "org.resonate.android.art", own))
        assertFalse(OpenedSources.mayOpen("content", "0@org.resonate.android.files", own))
        assertFalse(OpenedSources.mayOpen("CONTENT", "Org.Resonate.Android.Files", own))
    }

    @Test
    fun aProviderDeclaringSeveralAuthoritiesOwnsEach() {
        assertEquals(
            setOf("a.one", "a.two", "b"),
            OpenedSources.authoritiesOf(listOf("a.one;a.two", null, "b", "")),
        )
    }

    @Test
    fun aCopyWithinTheCeilingIsWhole() {
        val bytes = ByteArray(200_000) { it.toByte() }
        val copy = ByteArrayOutputStream()

        assertTrue(OpenedCopying.copiedWhole(ByteArrayInputStream(bytes), copy, bytes.size.toLong()))
        assertArrayEquals(bytes, copy.toByteArray())
    }

    @Test
    fun aCopyPastTheCeilingStops() {
        val bytes = ByteArray(200_000)
        val copy = ByteArrayOutputStream()

        assertFalse(OpenedCopying.copiedWhole(ByteArrayInputStream(bytes), copy, 199_999))
        assertTrue(copy.size() <= 199_999)
    }

    @Test
    fun theCeilingLeavesRoomFreeAndNeverPassesItsMost() {
        assertEquals(OpenedCopying.BYTES_AT_MOST, OpenedCopying.ceiling(Long.MAX_VALUE))
        assertEquals(5L, OpenedCopying.ceiling(OpenedCopying.LEFT_FREE_AT_LEAST + 5))
        assertEquals(0L, OpenedCopying.ceiling(OpenedCopying.LEFT_FREE_AT_LEAST - 5))
    }

    @Test
    fun twoFilesAreTheSameOnlyByteForByte() {
        val folder = Files.createTempDirectory("opened").toFile()
        try {
            val bytes = ByteArray(150_000) { (it % 251).toByte() }
            val first = File(folder, "a.flac").apply { writeBytes(bytes) }
            val same = File(folder, "b.flac").apply { writeBytes(bytes) }
            val changed =
                File(folder, "c.flac").apply { writeBytes(bytes.copyOf().also { it[140_000] = 1 }) }
            val shorter = File(folder, "d.flac").apply { writeBytes(bytes.copyOf(149_999)) }

            assertTrue(OpenedCopying.sameBytes(first, same))
            assertFalse(OpenedCopying.sameBytes(first, changed))
            assertFalse(OpenedCopying.sameBytes(first, shorter))
        } finally {
            folder.deleteRecursively()
        }
    }
}
