package org.resonate.android

import org.junit.Assert.assertEquals
import org.junit.Test

class OrganiseSentencesTest {
    @Test
    fun theConfirmationCountsTheSongsItMoves() {
        assertEquals("Move 1 song?", OrganiseSentences.confirmTitle(1))
        assertEquals("Move 12 songs?", OrganiseSentences.confirmTitle(12))
    }

    @Test
    fun theOutcomeCountsWhatMovedAndWhatCouldNot() {
        assertEquals("Filed 3 songs", OrganiseSentences.told(3, 0, undoing = false))
        assertEquals(
            "Filed 1 song; 2 songs could not be moved",
            OrganiseSentences.told(1, 2, undoing = false),
        )
        assertEquals(
            "Put 2 songs back where they were",
            OrganiseSentences.told(2, 0, undoing = true),
        )
        assertEquals(
            "Put 0 songs back where they were; 1 song could not be moved",
            OrganiseSentences.told(0, 1, undoing = true),
        )
    }

    @Test
    fun aSourceIsShownBeneathItsRootWithItsEscapingUndone() {
        val uri =
            "content://com.android.externalstorage.documents/tree/primary%3AMusic/document/" +
                "primary%3AMusic%2FDownloads%2F100%25%20Sure.flac"

        assertEquals("from Downloads / 100% Sure.flac", OrganiseSentences.shown(uri, "primary:Music"))
        assertEquals(
            "from primary:Other/x.flac",
            OrganiseSentences.shown("tree/a/document/primary%3AOther%2Fx.flac", "primary:Music"),
        )
    }
}
