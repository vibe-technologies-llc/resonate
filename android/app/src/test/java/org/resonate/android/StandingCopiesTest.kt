package org.resonate.android

import org.junit.Assert.assertEquals
import org.junit.Test

class StandingCopiesTest {
    private val homogenic = listOf("Björk", "Homogenic")

    private fun standingIn(tree: FakeTree, filed: List<List<String>>) =
        StandingCopies(tree).standing("Björk - Hunter.flac", filed).map {
            it.uri.removePrefix(FakeTree.SCHEME + FakeTree.ROOT + "/") to it.folder
        }

    @Test
    fun theRootHandsOnlyNamesOnTheStemAndAFiledFolderHandsEveryFile() {
        val tree =
            FakeTree().apply {
                add(
                    "Björk - Hunter.flac",
                    "Björk - Hunter (1).flac",
                    "Björk - Jóga.flac",
                    "Björk/Homogenic/02 Hunter.flac",
                    "Björk/Homogenic/cover.jpg",
                )
            }

        val standing = standingIn(tree, listOf(homogenic)).toSet()

        assertEquals(
            setOf(
                "Björk - Hunter.flac" to emptyList(),
                "Björk - Hunter (1).flac" to emptyList(),
                "Björk/Homogenic/02 Hunter.flac" to homogenic,
                "Björk/Homogenic/cover.jpg" to homogenic,
            ),
            standing,
        )
    }

    @Test
    fun aFolderNotNamedIsNeverWalked() {
        val tree =
            FakeTree().apply { add("Björk/Debut/Hunter.flac", "Björk/Hunter.flac", "Other/Hunter.flac") }

        assertEquals(emptyList<Pair<String, List<String>>>(), standingIn(tree, listOf(homogenic)))
        assertEquals(emptyList<Pair<String, List<String>>>(), standingIn(tree, emptyList()))
    }

    @Test
    fun aFolderSpelledDecomposedIsTheFolderNamedComposed() {
        val decomposed = "Björk"
        val tree = FakeTree().apply { add("$decomposed/Homogenic/02 Hunter.flac") }

        assertEquals(
            listOf("$decomposed/Homogenic/02 Hunter.flac" to homogenic),
            standingIn(tree, listOf(homogenic)),
        )
    }

    @Test
    fun aFolderLeavingTheTreeIsRefusedBeforeAnythingIsListed() {
        val tree = FakeTree().apply { add("Hunter.flac") }

        assertEquals(
            emptyList<Pair<String, List<String>>>(),
            standingIn(tree, listOf(listOf(".."), listOf(""))),
        )
    }

    @Test
    fun aFiledFolderInDownloadsIsItsRelativePath() {
        assertEquals(
            "Download/Björk/Homogenic/",
            StandingCopies.relativePath("Download", homogenic),
        )
    }
}
