package org.resonate.android

import org.junit.Assert.assertEquals
import org.junit.Test
import org.resonate.android.nativecore.HistoryBroughtIn

class HistoryWordsTest {
    @Test
    fun anImportSaysWhatLandedWhatWasHereAndWhatNamesSongsNotHere() {
        assertEquals(
            "Brought in 1 listen · 2 already here · 3 of songs not on this phone",
            historyBroughtInSaid(English, HistoryBroughtIn(1uL, 2uL, 3uL)),
        )
        assertEquals(
            "Brought in 4 listens · 0 already here · 0 of songs not on this phone",
            historyBroughtInSaid(English, HistoryBroughtIn(4uL, 0uL, 0uL)),
        )
    }
}
