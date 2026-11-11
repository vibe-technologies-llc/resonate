package org.resonate.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class OpenedNamesTest {
    private val unnamed = English.string(R.string.opened_unnamed)

    @Test
    fun aDocumentIsInTheTreeThatHoldsItAndNoOtherWithTheSamePrefix() {
        assertTrue(OpenedNames.withinTree("primary:Music/a/b.flac", "primary:Music"))
        assertTrue(OpenedNames.withinTree("primary:Music", "primary:Music"))
        assertFalse(OpenedNames.withinTree("primary:Music2/b.flac", "primary:Music"))
        assertTrue(OpenedNames.withinTree("primary:Music/b.flac", "primary:"))
    }

    @Test
    fun twoTreesOverlapWhenOneHoldsTheOtherAndNotWhenTheyAreTheSameOrApart() {
        assertTrue(OpenedNames.treesOverlap("primary:Music", "primary:Music/Albums"))
        assertTrue(OpenedNames.treesOverlap("primary:Music/Albums", "primary:Music"))
        assertFalse(OpenedNames.treesOverlap("primary:Music", "primary:Music"))
        assertFalse(OpenedNames.treesOverlap("primary:Music", "primary:Music2"))
        assertFalse(OpenedNames.treesOverlap("primary:Music", "primary:Podcasts"))
    }

    @Test
    fun aTakenNameGainsANumberBeforeItsExtension() {
        assertEquals("a.flac", OpenedNames.nameAmong("a.flac", setOf("b.flac")))
        assertEquals("a (2).flac", OpenedNames.nameAmong("a.flac", setOf("a.flac")))
        assertEquals("a (3).flac", OpenedNames.nameAmong("a.flac", setOf("a.flac", "a (2).flac")))
        assertEquals("a (2)", OpenedNames.nameAmong("a", setOf("a")))
    }

    @Test
    fun theCopiesOfANameAreItAndItsNumberedTakenNames() {
        assertEquals(
            listOf("a.flac", "a (2).flac", "a (4).flac"),
            OpenedNames.copiesOf(
                "a.flac",
                setOf("a.flac", "a (2).flac", "a (4).flac", "b.flac", "a (2).mp3"),
            ),
        )
        assertEquals(listOf("a (3).flac"), OpenedNames.copiesOf("a.flac", setOf("a (3).flac")))
        assertEquals(emptyList<String>(), OpenedNames.copiesOf("a.flac", setOf("b.flac")))
    }

    @Test
    fun aNameLosesWhatAFileSystemRefuses() {
        assertEquals("a_b_.flac", OpenedNames.safeName("a:b?.flac", unnamed))
        assertEquals("c.flac", OpenedNames.safeName("x/y/c.flac", unnamed))
        assertEquals("Opened song", OpenedNames.safeName(null, unnamed))
        assertEquals("Opened song", OpenedNames.safeName("  ", unnamed))
        assertEquals("Opened song", OpenedNames.safeName("..", unnamed))
        assertEquals("Opened song", OpenedNames.safeName("x/.", unnamed))
    }

    @Test
    fun aLocalSongKeepsItsEscapingThereAndBack() {
        val uri = OpenedNames.localUri("100% a b.flac")

        assertEquals("resonate-folder://local/100%25%20a%20b.flac", uri)
        assertEquals("100% a b.flac", OpenedNames.localName(uri))
        assertEquals(null, OpenedNames.localName("content://x/y"))
    }

    @Test
    fun aLocalNameNeverReachesOutOfTheFolder() {
        assertEquals(null, OpenedNames.localName("resonate-folder://local/..%2Fshared_prefs%2Fa.xml"))
        assertEquals(null, OpenedNames.localName("resonate-folder://local/../a"))
        assertEquals(null, OpenedNames.localName("resonate-folder://local/.."))
        assertEquals(null, OpenedNames.localName("resonate-folder://local/"))
        assertEquals("..a.flac", OpenedNames.localName("resonate-folder://local/..a.flac"))
    }
}
