package org.resonate.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class OpenedNamesTest {
    @Test
    fun aDocumentIsInTheTreeThatHoldsItAndNoOtherWithTheSamePrefix() {
        assertTrue(OpenedNames.withinTree("primary:Music/a/b.flac", "primary:Music"))
        assertTrue(OpenedNames.withinTree("primary:Music", "primary:Music"))
        assertFalse(OpenedNames.withinTree("primary:Music2/b.flac", "primary:Music"))
        assertTrue(OpenedNames.withinTree("primary:Music/b.flac", "primary:"))
    }

    @Test
    fun aTakenNameGainsANumberBeforeItsExtension() {
        assertEquals("a.flac", OpenedNames.nameAmong("a.flac", setOf("b.flac")))
        assertEquals("a (2).flac", OpenedNames.nameAmong("a.flac", setOf("a.flac")))
        assertEquals("a (3).flac", OpenedNames.nameAmong("a.flac", setOf("a.flac", "a (2).flac")))
        assertEquals("a (2)", OpenedNames.nameAmong("a", setOf("a")))
    }

    @Test
    fun aNameLosesWhatAFileSystemRefuses() {
        assertEquals("a_b_.flac", OpenedNames.safeName("a:b?.flac"))
        assertEquals("c.flac", OpenedNames.safeName("x/y/c.flac"))
        assertEquals("Opened song", OpenedNames.safeName(null))
        assertEquals("Opened song", OpenedNames.safeName("  "))
    }

    @Test
    fun aLocalSongKeepsItsEscapingThereAndBack() {
        val uri = OpenedNames.localUri("100% a b.flac")

        assertEquals("resonate-folder://local/100%25%20a%20b.flac", uri)
        assertEquals("100% a b.flac", OpenedNames.localName(uri))
        assertEquals(null, OpenedNames.localName("content://x/y"))
    }
}
