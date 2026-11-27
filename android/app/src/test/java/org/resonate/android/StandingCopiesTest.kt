package org.resonate.android

import org.junit.Assert.assertEquals
import org.junit.Test

class StandingCopiesTest {
    private val seabrook = listOf("Sölvi", "Seabrook")

    private fun standingIn(tree: FakeTree, filed: List<List<String>>) =
        StandingCopies(tree).standing("Sölvi - Wavebreak.flac", filed).map {
            it.uri.removePrefix(FakeTree.SCHEME + FakeTree.ROOT + "/") to it.folder
        }

    @Test
    fun theRootHandsOnlyNamesOnTheStemAndAFiledFolderHandsEveryFile() {
        val tree =
            FakeTree().apply {
                add(
                    "Sölvi - Wavebreak.flac",
                    "Sölvi - Wavebreak (1).flac",
                    "Sölvi - Lúna.flac",
                    "Sölvi/Seabrook/02 Wavebreak.flac",
                    "Sölvi/Seabrook/cover.jpg",
                )
            }

        val standing = standingIn(tree, listOf(seabrook)).toSet()

        assertEquals(
            setOf(
                "Sölvi - Wavebreak.flac" to emptyList(),
                "Sölvi - Wavebreak (1).flac" to emptyList(),
                "Sölvi/Seabrook/02 Wavebreak.flac" to seabrook,
                "Sölvi/Seabrook/cover.jpg" to seabrook,
            ),
            standing,
        )
    }

    @Test
    fun aFolderNotNamedIsNeverWalked() {
        val tree =
            FakeTree().apply { add("Sölvi/Early Days/Wavebreak.flac", "Sölvi/Wavebreak.flac", "Other/Wavebreak.flac") }

        assertEquals(emptyList<Pair<String, List<String>>>(), standingIn(tree, listOf(seabrook)))
        assertEquals(emptyList<Pair<String, List<String>>>(), standingIn(tree, emptyList()))
    }

    @Test
    fun aFolderSpelledDecomposedIsTheFolderNamedComposed() {
        val decomposed = "Sölvi"
        val tree = FakeTree().apply { add("$decomposed/Seabrook/02 Wavebreak.flac") }

        assertEquals(
            listOf("$decomposed/Seabrook/02 Wavebreak.flac" to seabrook),
            standingIn(tree, listOf(seabrook)),
        )
    }

    @Test
    fun aFolderLeavingTheTreeIsRefusedBeforeAnythingIsListed() {
        val tree = FakeTree().apply { add("Wavebreak.flac") }

        assertEquals(
            emptyList<Pair<String, List<String>>>(),
            standingIn(tree, listOf(listOf(".."), listOf(""))),
        )
    }

    @Test
    fun aFiledFolderInDownloadsIsItsRelativePath() {
        assertEquals(
            "Download/Sölvi/Seabrook/",
            StandingCopies.relativePath("Download", seabrook),
        )
    }
}
