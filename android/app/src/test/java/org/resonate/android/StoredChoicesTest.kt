package org.resonate.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.resonate.android.nativecore.KeptHistory

class StoredChoicesTest {
    private fun stored(vararg held: Pair<String, Boolean>): (String) -> Boolean? = mapOf(*held)::get

    @Test
    fun aCopyOfMusicbrainzIsTakenUnlessChosenOffWhateverTheFirstSwitchSaved() {
        assertTrue(StoredChoices.musicbrainzCopy(stored()))
        assertTrue(StoredChoices.musicbrainzCopy(stored("sync-musicbrainz-copy" to false)))
        assertTrue(StoredChoices.musicbrainzCopy(stored("sync-musicbrainz-copy-chosen" to true)))
        assertFalse(StoredChoices.musicbrainzCopy(stored("sync-musicbrainz-copy-chosen" to false)))
        assertFalse(
            StoredChoices.musicbrainzCopy(
                stored("sync-musicbrainz-copy" to true, "sync-musicbrainz-copy-chosen" to false),
            ),
        )
    }

    @Test
    fun downloadsGoOnWhileMeteredUnlessTurnedOffAndAStallSavedBeforeReadsTheOtherWayRound() {
        assertTrue(StoredChoices.downloadOnMetered(stored()))
        assertFalse(StoredChoices.downloadOnMetered(stored("stall-downloads-on-metered" to true)))
        assertTrue(StoredChoices.downloadOnMetered(stored("stall-downloads-on-metered" to false)))
        assertFalse(StoredChoices.downloadOnMetered(stored("download-on-metered" to false)))
        assertTrue(
            StoredChoices.downloadOnMetered(
                stored("stall-downloads-on-metered" to true, "download-on-metered" to true),
            ),
        )
    }

    @Test
    fun otherDevicesMayDeleteSongsHereUnlessTurnedOff() {
        assertTrue(StoredChoices.deletionsFromOthers(stored()))
        assertTrue(StoredChoices.deletionsFromOthers(stored("sync-deletions-from-others" to true)))
        assertFalse(StoredChoices.deletionsFromOthers(stored("sync-deletions-from-others" to false)))
    }

    @Test
    fun newerReleasesAreLookedForUnlessTurnedOff() {
        assertTrue(StoredChoices.checkForUpdates(stored()))
        assertTrue(StoredChoices.checkForUpdates(stored("check-for-updates" to true)))
        assertFalse(StoredChoices.checkForUpdates(stored("check-for-updates" to false)))
    }

    @Test
    fun theHistoryIsKeptForeverUntilASpanIsSavedAndOnlyAShorterSpanForgetsListens() {
        val year = KeptHistory.Days(365u)
        val halfYear = KeptHistory.Days(182u)

        assertEquals(KeptHistory.Forever, StoredChoices.historyKept(mapOf<String, Int>()::get))
        assertEquals(year, StoredChoices.historyKept(mapOf("history-kept-days" to 365)::get))
        assertEquals(KeptHistory.Forever, StoredChoices.historyKept(mapOf("history-kept-days" to 0)::get))
        assertEquals(365, StoredChoices.historyKeptDays(year))
        assertEquals(null, StoredChoices.historyKeptDays(KeptHistory.Forever))
        assertTrue(StoredChoices.forgetsListens(KeptHistory.Forever, year))
        assertTrue(StoredChoices.forgetsListens(year, halfYear))
        assertFalse(StoredChoices.forgetsListens(halfYear, year))
        assertFalse(StoredChoices.forgetsListens(year, KeptHistory.Forever))
        assertFalse(StoredChoices.forgetsListens(KeptHistory.Forever, KeptHistory.Forever))
    }
}
