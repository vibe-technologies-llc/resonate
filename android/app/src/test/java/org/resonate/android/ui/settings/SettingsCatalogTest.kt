package org.resonate.android.ui.settings

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class SettingsCatalogTest {
    private fun found(query: String) = matchingSettings(query).map { it.setting }

    @Test
    fun anEmptyQueryShowsEverySettingWithItsOwnDetail() {
        val all = matchingSettings("  ")

        assertEquals(Setting.entries.toList(), all.map { it.setting })
        assertTrue(all.all { it.shownDetail == it.setting.detail })
    }

    @Test
    fun aTitleMatchesWhateverItsCase() {
        assertEquals(listOf(Setting.EQUALISER), found("EQUAL"))
        assertEquals(listOf(Setting.SCROBBLING), found("scrob"))
    }

    @Test
    fun everyWordMustMatchTheStartOfSomeWord() {
        assertEquals(listOf(Setting.HEADPHONE_CORRECTION), found("headphone corr"))
        assertTrue(found("phone").isEmpty())
    }

    @Test
    fun anInnerSettingIsFoundAndNamedInPlaceOfTheDetail() {
        val match = Setting.LIBRARY_TOOLS.match("wifi")

        assertEquals("Only on Wi-Fi", match?.shownDetail)
        assertEquals(listOf(Setting.LIBRARY_TOOLS), found("wi-fi"))
    }

    @Test
    fun hyphenatedWordsAnswerToEitherHalfOrTheWhole() {
        assertEquals("True-peak clip prevention", Setting.PROCESSING.match("true peak")?.shownDetail)
        assertEquals("True-peak clip prevention", Setting.PROCESSING.match("truepeak")?.shownDetail)
        assertEquals("True-peak clip prevention", Setting.PROCESSING.match("peak")?.shownDetail)
    }

    @Test
    fun accentsAreFoldedAway() {
        assertEquals(listOf(Setting.EQUALISER), found("équaliser"))
    }

    @Test
    fun wordsSpreadAcrossTitleAndInnerSettingsStillMatchWithTheDetail() {
        val match = Setting.SYNC.match("devices pairing")

        assertEquals(Setting.SYNC.detail, match?.shownDetail)
    }

    @Test
    fun theGroupNameFindsItsSettings() {
        assertTrue(found("sound").containsAll(listOf(Setting.EQUALISER, Setting.OUTPUT)))
    }

    @Test
    fun nothingMatchesNonsense() {
        assertNull(Setting.OUTPUT.match("zzzz"))
        assertTrue(found("zzzz").isEmpty())
    }

    @Test
    fun mixesAreFoundUnderMadeForYou() {
        assertEquals("High resolution", Setting.SUGGESTIONS.match("resolution")?.shownDetail)
    }
}
