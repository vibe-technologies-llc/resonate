package org.resonate.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.resonate.android.nativecore.FiledSong
import org.resonate.android.nativecore.MovedDocument

private class FakeTree(override val rootId: String = ROOT) : DocumentTree {
    val files = LinkedHashSet<String>()
    val directories = linkedSetOf(rootId)
    val removed = ArrayList<String>()
    var refuseRename = false
    var unlistable: String? = null

    fun add(vararg paths: String) {
        for (path in paths) {
            val id = "$rootId/$path"
            var parent = Ids.parent(id)
            while (parent != rootId) {
                directories.add(parent)
                parent = Ids.parent(parent)
            }
            files.add(id)
        }
    }

    fun has(path: String) = "$rootId/$path" in files

    fun hasDirectory(path: String) = "$rootId/$path" in directories

    override fun documentId(uri: String): String? = uri.removePrefix(SCHEME).takeIf { uri.startsWith(SCHEME) }

    override fun uri(documentId: String): String = SCHEME + documentId

    override fun children(directoryId: String): List<TreeEntry>? {
        if (directoryId == unlistable) return null
        return (directories.map { it to true } + files.map { it to false })
            .filter { (id, _) -> id != directoryId && Ids.parent(id) == directoryId }
            .map { (id, directory) -> TreeEntry(id, id.substringAfterLast('/'), directory) }
    }

    override fun makeDirectory(parentId: String, name: String): String? {
        val id = "$parentId/$name"
        if (id in directories || id in files) return null
        directories.add(id)
        return id
    }

    override fun move(documentId: String, fromId: String, toId: String): String? {
        val id = "$toId/${documentId.substringAfterLast('/')}"
        if (documentId !in files || id in files) return null
        files.remove(documentId)
        files.add(id)
        return id
    }

    override fun rename(documentId: String, name: String): String? {
        val id = "${Ids.parent(documentId)}/$name"
        if (refuseRename || documentId !in files || id in files) return null
        files.remove(documentId)
        files.add(id)
        return id
    }

    override fun delete(documentId: String): Boolean {
        removed.add(documentId)
        return directories.remove(documentId)
    }

    companion object {
        const val ROOT = "primary:Music"
        const val SCHEME = "tree:"
    }
}

private fun filed(id: Long, from: String, vararg to: String) =
    FiledSong(id.toULong(), FakeTree.SCHEME + "${FakeTree.ROOT}/$from", to.toList())

class FolderFilingTest {
    @Test
    fun aSongMovesIntoFoldersMadeForItAndIsReportedAtTheUriTheProviderGave() {
        val tree = FakeTree().apply { add("Downloads/flesh.flac") }

        val result =
            FolderFiling(tree)
                .file(listOf(filed(1, "Downloads/flesh.flac", "Pink Floyd", "The Wall", "01 Flesh.flac")))

        assertEquals(0, result.failed)
        assertTrue(tree.has("Pink Floyd/The Wall/01 Flesh.flac"))
        assertFalse(tree.has("Downloads/flesh.flac"))
        assertEquals(
            listOf(MovedDocument(1uL, "tree:primary:Music/Pink Floyd/The Wall/01 Flesh.flac")),
            result.moved,
        )
        assertEquals(
            listOf(
                FiledSong(
                    1uL,
                    "tree:primary:Music/Pink Floyd/The Wall/01 Flesh.flac",
                    listOf("Downloads", "flesh.flac"),
                )
            ),
            result.back,
        )
    }

    @Test
    fun aSongRenamedWhereItStandsKeepsItsFolder() {
        val tree = FakeTree().apply { add("Pink Floyd/The Wall/flesh.flac") }

        val result =
            FolderFiling(tree)
                .file(listOf(filed(1, "Pink Floyd/The Wall/flesh.flac", "Pink Floyd", "The Wall", "01 Flesh.flac")))

        assertEquals(0, result.failed)
        assertTrue(tree.has("Pink Floyd/The Wall/01 Flesh.flac"))
        assertFalse(tree.has("Pink Floyd/The Wall/flesh.flac"))
        assertTrue(tree.removed.isEmpty())
    }

    @Test
    fun aPlaceThatIsHeldCountsAsFailedAndMovesNothing() {
        val tree =
            FakeTree().apply {
                add("a.flac", "Pink Floyd/The Wall/01 Flesh.flac")
            }

        val result = FolderFiling(tree).file(listOf(filed(1, "a.flac", "Pink Floyd", "The Wall", "01 flesh.FLAC")))

        assertEquals(1, result.failed)
        assertTrue(result.moved.isEmpty())
        assertTrue(result.back.isEmpty())
        assertTrue(tree.has("a.flac"))
    }

    @Test
    fun aSourceOutsideTheRootOrOfAnotherTreeCountsAsFailed() {
        val tree = FakeTree().apply { add("a.flac") }
        tree.files.add("primary:Other/b.flac")

        val outside = FiledSong(2uL, "tree:primary:Other/b.flac", listOf("b.flac"))
        val foreign = FiledSong(3uL, "content://elsewhere/x", listOf("x.flac"))
        val root = FiledSong(4uL, "tree:primary:Music", listOf("x.flac"))
        val result = FolderFiling(tree).file(listOf(outside, foreign, root))

        assertEquals(3, result.failed)
        assertTrue(result.moved.isEmpty())
        assertTrue("primary:Other/b.flac" in tree.files)
    }

    @Test
    fun aSourceThatIsNotThereOrAnUnsafeNameCountsAsFailed() {
        val tree = FakeTree().apply { add("a.flac") }

        val missing = filed(1, "gone.flac", "x", "gone.flac")
        val climbing = filed(2, "a.flac", "..", "a.flac")
        val slashed = filed(3, "a.flac", "x/y.flac")
        val empty = FiledSong(4uL, "tree:primary:Music/a.flac", emptyList())
        val result = FolderFiling(tree).file(listOf(missing, climbing, slashed, empty))

        assertEquals(4, result.failed)
        assertTrue(tree.has("a.flac"))
    }

    @Test
    fun aSidecarLyricMovesAlongWhereItsPlaceIsFree() {
        val tree = FakeTree().apply { add("In/flesh.flac", "In/Flesh.LRC", "In/other.lrc") }

        FolderFiling(tree).file(listOf(filed(1, "In/flesh.flac", "Out", "01 Flesh.flac")))

        assertTrue(tree.has("Out/01 Flesh.lrc"))
        assertFalse(tree.has("In/Flesh.LRC"))
        assertTrue(tree.has("In/other.lrc"))
    }

    @Test
    fun aSidecarStaysWhereItsPlaceIsHeld() {
        val tree =
            FakeTree().apply { add("In/flesh.flac", "In/flesh.lrc", "Out/01 flesh.LRC") }

        val result = FolderFiling(tree).file(listOf(filed(1, "In/flesh.flac", "Out", "01 Flesh.flac")))

        assertEquals(0, result.failed)
        assertTrue(tree.has("In/flesh.lrc"))
        assertTrue(tree.has("Out/01 flesh.LRC"))
    }

    @Test
    fun foldersTheMovesLeftEmptyAreRemovedUpToTheRootAndNoFurther() {
        val tree =
            FakeTree().apply {
                add("Loose/Deep/a.flac", "Loose/Deep/b.flac", "Kept/c.flac", "Kept/cover.jpg", "Shelf/d.flac")
            }

        FolderFiling(tree)
            .file(
                listOf(
                    filed(1, "Loose/Deep/a.flac", "A", "a.flac"),
                    filed(2, "Loose/Deep/b.flac", "A", "b.flac"),
                    filed(3, "Kept/c.flac", "A", "c.flac"),
                    filed(4, "Shelf/d.flac", "Shelf", "d.flac"),
                )
            )

        assertFalse(tree.hasDirectory("Loose/Deep"))
        assertFalse(tree.hasDirectory("Loose"))
        assertTrue(tree.hasDirectory("Kept"))
        assertTrue(tree.has("Kept/cover.jpg"))
        assertTrue(tree.hasDirectory("Shelf"))
        assertTrue(FakeTree.ROOT in tree.directories)
        assertFalse("primary:Music" in tree.removed)
    }

    @Test
    fun aFolderThatCannotBeListedIsNeverRemoved() {
        val tree = FakeTree().apply { add("Loose/a.flac", "Loose/keep.txt") }
        tree.unlistable = "primary:Music/Loose"

        val result = FolderFiling(tree).file(listOf(filed(1, "Loose/a.flac", "A", "a.flac")))

        assertEquals(1, result.failed)
        assertTrue(tree.removed.isEmpty())
        assertTrue(tree.has("Loose/keep.txt"))
    }

    @Test
    fun puttingTheFilesBackReturnsEverySongToItsOldPlaceAndTidiesTheNewFolders() {
        val tree = FakeTree().apply { add("Loose/Deep/a.flac", "Loose/Deep/a.lrc", "b.flac") }
        val first =
            FolderFiling(tree)
                .file(
                    listOf(
                        filed(1, "Loose/Deep/a.flac", "Pink Floyd", "The Wall", "01 A.flac"),
                        filed(2, "b.flac", "Pink Floyd", "The Wall", "02 B.flac"),
                    )
                )
        assertTrue(tree.has("Pink Floyd/The Wall/01 A.lrc"))
        assertFalse(tree.hasDirectory("Loose"))

        val second = FolderFiling(tree).file(first.back)

        assertEquals(0, second.failed)
        assertTrue(tree.has("Loose/Deep/a.flac"))
        assertTrue(tree.has("Loose/Deep/a.lrc"))
        assertTrue(tree.has("b.flac"))
        assertFalse(tree.hasDirectory("Pink Floyd"))
        assertEquals(
            listOf(MovedDocument(2uL, "tree:primary:Music/b.flac"), MovedDocument(1uL, "tree:primary:Music/Loose/Deep/a.flac")).toSet(),
            second.moved.toSet(),
        )
    }

    @Test
    fun aMoveWhoseRenameIsRefusedPutsTheFileBackWhereItWas() {
        val tree = FakeTree().apply { add("In/flesh.flac") }
        tree.refuseRename = true

        val result = FolderFiling(tree).file(listOf(filed(1, "In/flesh.flac", "Out", "01 Flesh.flac")))

        assertEquals(1, result.failed)
        assertTrue(tree.has("In/flesh.flac"))
        assertFalse(tree.has("Out/flesh.flac"))
        assertFalse(tree.hasDirectory("Out"))
    }

    @Test
    fun aNameTheDestinationFolderAlreadyUsesIsRenamedBeforeTheMove() {
        val tree = FakeTree().apply { add("In/flesh.flac", "Out/flesh.flac") }

        val result = FolderFiling(tree).file(listOf(filed(1, "In/flesh.flac", "Out", "01 Flesh.flac")))

        assertEquals(0, result.failed)
        assertTrue(tree.has("Out/01 Flesh.flac"))
        assertTrue(tree.has("Out/flesh.flac"))
        assertFalse(tree.has("In/flesh.flac"))
    }

    @Test
    fun theSecondOfTwoSongsMeetingAtOnePlaceFindsItHeld() {
        val tree = FakeTree().apply { add("a.flac", "b.flac") }

        val result =
            FolderFiling(tree)
                .file(listOf(filed(1, "a.flac", "A", "x.flac"), filed(2, "b.flac", "A", "x.flac")))

        assertEquals(1, result.failed)
        assertEquals(listOf(1uL), result.moved.map { it.id })
        assertTrue(tree.has("b.flac"))
    }

    @Test
    fun aDocumentIdBelowTheRootGivesItsComponentsAndNothingElseDoes() {
        assertEquals(listOf("a", "b.flac"), Ids.relative("primary:Music/a/b.flac", "primary:Music"))
        assertNull(Ids.relative("primary:Music", "primary:Music"))
        assertNull(Ids.relative("primary:Music2/a", "primary:Music"))
        assertNull(Ids.relative("primary:Other/a", "primary:Music"))
        assertEquals(listOf("a.flac"), Ids.relative("primary:a.flac", "primary:"))
    }

    @Test
    fun aNameIsSafeOnlyWhenEveryComponentIsAPlainName() {
        assertTrue(Ids.nameable(listOf("A", "b.flac")))
        assertFalse(Ids.nameable(emptyList()))
        assertFalse(Ids.nameable(listOf("", "b.flac")))
        assertFalse(Ids.nameable(listOf(".", "b.flac")))
        assertFalse(Ids.nameable(listOf("a/b", "c.flac")))
    }

    @Test
    fun aSidecarShadowsTheSongsStem() {
        assertEquals("01 A.lrc", Ids.sidecarOf("01 A.flac"))
        assertEquals("a.b.lrc", Ids.sidecarOf("a.b.flac"))
        assertEquals("noext.lrc", Ids.sidecarOf("noext"))
    }
}
