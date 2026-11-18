package org.resonate.android

import org.junit.Assert.assertEquals
import org.junit.Test
import org.resonate.android.nativecore.HistoryBroughtIn
import org.resonate.android.nativecore.KeptHistory
import org.resonate.android.ui.settings.historyKeptLabel

class HistoryWordsTest {
    @Test
    fun eachSpanTheDesktopOffersIsNamedAsItNamesItAndAnyOtherByItsDays() {
        assertEquals("Forever", historyKeptLabel(English, KeptHistory.Forever))
        assertEquals("Five years", historyKeptLabel(English, KeptHistory.Days(1826u)))
        assertEquals("Two years", historyKeptLabel(English, KeptHistory.Days(730u)))
        assertEquals("A year", historyKeptLabel(English, KeptHistory.Days(365u)))
        assertEquals("Six months", historyKeptLabel(English, KeptHistory.Days(182u)))
        assertEquals("1 day", historyKeptLabel(English, KeptHistory.Days(1u)))
        assertEquals("90 days", historyKeptLabel(English, KeptHistory.Days(90u)))
    }

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
