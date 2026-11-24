package org.resonate.android

import java.io.FileNotFoundException
import org.junit.Assert.assertEquals
import org.junit.Test

private class ListedTree(
    override val rootId: String,
    private val listings: Map<String, List<String>?>,
) : DocumentTree {
    val asked = ArrayList<String>()

    override fun documentId(uri: String): String? = uri

    override fun uri(documentId: String): String = documentId

    override fun children(directoryId: String): List<TreeEntry>? {
        asked.add(directoryId)
        return listings[directoryId]?.map { TreeEntry(it, it.substringAfterLast('/'), false) }
    }

    override fun makeDirectory(parentId: String, name: String): String? = null

    override fun move(documentId: String, fromId: String, toId: String): String? = null

    override fun rename(documentId: String, name: String): String? = null

    override fun delete(documentId: String): Boolean = false
}

class DocumentPresenceTest {
    private val root = "primary:Music"
    private val album = "$root/Album"
    private val song = "$album/a.flac"

    private fun openedIn(tree: DocumentTree, thrown: Exception = FileNotFoundException()) =
        DocumentPresence.failure(thrown) { DocumentPresence.inTree(tree, song) }

    @Test
    fun aDocumentMissingFromAFolderThatListsIsNotFound() {
        val tree = ListedTree(root, mapOf(album to listOf("$album/b.flac")))

        assertEquals(RowPresence.Absent, DocumentPresence.inTree(tree, song))
        assertEquals(OpenFailure.NotFound, openedIn(tree))
        assertEquals(OpenFailure.NotFound, openedIn(tree, IllegalArgumentException()))
        assertEquals(listOf(album, album, album), tree.asked)
    }

    @Test
    fun aFolderGoneFromAFolderThatListsTakesItsSongsWithIt() {
        val tree = ListedTree(root, mapOf(root to listOf("$root/Other")))

        assertEquals(RowPresence.Absent, DocumentPresence.inTree(tree, song))
        assertEquals(OpenFailure.NotFound, openedIn(tree))
    }

    @Test
    fun aDocumentTheFolderStillListsIsRefusedThoughItsOpenSaidNotFound() {
        val tree = ListedTree(root, mapOf(album to listOf(song)))

        assertEquals(RowPresence.Standing, DocumentPresence.inTree(tree, song))
        assertEquals(OpenFailure.Refused, openedIn(tree))
    }

    @Test
    fun anUnavailableSubfolderLeavesItsSongsRefused() {
        val tree = ListedTree(root, mapOf(root to listOf(album), album to null))

        assertEquals(RowPresence.Unknown, DocumentPresence.inTree(tree, song))
        assertEquals(OpenFailure.Refused, openedIn(tree))
        assertEquals(OpenFailure.Refused, openedIn(tree, IllegalArgumentException()))
    }

    @Test
    fun aProviderThatListsNothingLeavesEverySongRefused() {
        val tree = ListedTree(root, emptyMap())

        assertEquals(RowPresence.Unknown, DocumentPresence.inTree(tree, song))
        assertEquals(OpenFailure.Refused, openedIn(tree))
    }

    @Test
    fun aRevokedGrantIsRefusedWithoutAskingWhereTheDocumentIs() {
        val answer = DocumentPresence.failure(SecurityException()) { error("asked") }

        assertEquals(OpenFailure.Refused, answer)
    }

    @Test
    fun aPresenceThatCannotBeReadIsRefused() {
        val answer =
            DocumentPresence.failure(FileNotFoundException()) {
                throw IllegalStateException()
            }

        assertEquals(OpenFailure.Refused, answer)
    }

    @Test
    fun aDocumentTheTreeDoesNotNameByPathIsNeverAbsent() {
        val tree = ListedTree(root, mapOf(root to emptyList()))

        assertEquals(RowPresence.Unknown, DocumentPresence.inTree(tree, "acc=1;doc=42"))
        assertEquals(RowPresence.Unknown, DocumentPresence.inTree(tree, root))
    }

    @Test
    fun aWholeVolumeTreeReachesItsSongsByTheirPath() {
        val volume = "primary:"
        val tree =
            ListedTree(
                volume,
                mapOf(volume to listOf("primary:Album"), "primary:Album" to emptyList()),
            )

        assertEquals(RowPresence.Absent, DocumentPresence.inTree(tree, "primary:Album/a.flac"))
        assertEquals(RowPresence.Absent, DocumentPresence.inTree(tree, "primary:Gone/a.flac"))
        assertEquals(RowPresence.Standing, DocumentPresence.inTree(tree, "primary:Album"))
    }
}
