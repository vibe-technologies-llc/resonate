package org.resonate.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class DeletionSentencesTest {
    @Test
    fun aLoneSongIsNamedAndSeveralAreCounted() {
        assertEquals("Delete “Salt Glass”?", DeletionSentences.title(English, listOf("Salt Glass")))
        assertEquals("Delete 3 songs?", DeletionSentences.title(English, listOf("a", "b", "c")))
        assertEquals("Deleted “Salt Glass”", DeletionSentences.deleted(English, listOf("Salt Glass")))
        assertEquals("Deleted 2 songs", DeletionSentences.deleted(English, listOf("a", "b")))
    }

    @Test
    fun filesThatStayAreToldByTheirNumber() {
        assertEquals(
            "1 file could not be deleted; its folder would not let go of it",
            DeletionSentences.kept(English, 1),
        )
        assertEquals(
            "2 files could not be deleted; their folders would not let go of them",
            DeletionSentences.kept(English, 2),
        )
    }

    @Test
    fun foldedCopiesAreCounted() {
        assertEquals("Deleted 1 identical file; each song keeps one", DeletionSentences.folded(English, 1))
        assertEquals("Deleted 4 identical files; each song keeps one", DeletionSentences.folded(English, 4))
    }

    @Test
    fun copiesNoLongerConfirmedAreToldBesideFilesAFolderKept() {
        assertEquals(
            "2 copies were kept; they could no longer be confirmed as copies of the song kept",
            DeletionSentences.stayed(English, Deleted(0, 0, unconfirmed = 2)),
        )
        assertEquals(
            "1 file could not be deleted; its folder would not let go of it; " +
                "1 copy was kept; it could no longer be confirmed as a copy of the song kept",
            DeletionSentences.stayed(English, Deleted(0, 1, folded = 3, unconfirmed = 1)),
        )
        assertNull(DeletionSentences.stayed(English, Deleted(0, 0, folded = 3)))
    }
}
