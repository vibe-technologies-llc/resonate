package org.resonate.android

import org.junit.Assert.assertEquals
import org.junit.Test

class DeletionSentencesTest {
    @Test
    fun aLoneSongIsNamedAndSeveralAreCounted() {
        assertEquals("Delete “Salt Glass”?", DeletionSentences.title(listOf("Salt Glass")))
        assertEquals("Delete 3 songs?", DeletionSentences.title(listOf("a", "b", "c")))
        assertEquals("Deleted “Salt Glass”", DeletionSentences.deleted(listOf("Salt Glass")))
        assertEquals("Deleted 2 songs", DeletionSentences.deleted(listOf("a", "b")))
    }

    @Test
    fun filesThatStayAreToldByTheirNumber() {
        assertEquals(
            "1 file could not be deleted; its folder would not let go of it",
            DeletionSentences.kept(1),
        )
        assertEquals(
            "2 files could not be deleted; their folders would not let go of them",
            DeletionSentences.kept(2),
        )
    }

    @Test
    fun foldedCopiesAreCounted() {
        assertEquals("Deleted 1 identical file; each song keeps one", DeletionSentences.folded(1))
        assertEquals("Deleted 4 identical files; each song keeps one", DeletionSentences.folded(4))
    }
}
