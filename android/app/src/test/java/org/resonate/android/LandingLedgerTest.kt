package org.resonate.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class LandingLedgerTest {
    private val into = LandingDestination.Into("music")
    private val song = 7uL
    private val document = "content://media/external/file/42"

    @Test
    fun aRejectedPublicationIsRetriedLaterWithoutTheSongBeingRefused() {
        val ledger = LandingLedger()

        ledger.notPublished(document, 0L, "not made available")

        assertEquals(1, ledger.unpublishedCount)
        assertTrue(ledger.isDue(song, 0L))
        assertTrue(ledger.duePublications(LandingLedger.PUBLICATION_PAUSE_MS - 1).isEmpty())
        assertEquals(listOf(document), ledger.duePublications(LandingLedger.PUBLICATION_PAUSE_MS))

        ledger.published(document)

        assertEquals(0, ledger.unpublishedCount)
        assertTrue(ledger.duePublications(Long.MAX_VALUE).isEmpty())
    }

    @Test
    fun aPublicationRefusedPastTheBoundStopsBeingDueAndLaterLandingsProceed() {
        val ledger = LandingLedger()
        val pause = LandingLedger.PUBLICATION_PAUSE_MS
        val tries = LandingLedger.PUBLICATION_TRIES_AT_MOST

        for (attempt in 1 until tries) {
            assertFalse(ledger.notPublished(document, attempt * pause, "not made available"))
            assertEquals(1, ledger.unpublishedCount)
        }
        val abandoned = ledger.notPublished(document, tries * pause, "not made available")

        assertTrue(abandoned)
        assertEquals(0, ledger.unpublishedCount)
        assertTrue(ledger.duePublications(Long.MAX_VALUE).isEmpty())
        assertFalse(ledger.work(0, into, false).transferring)
        assertNull(ledger.work(0, into, false).notice(English))
        assertTrue(ledger.work(2, into, false).progressing)
        assertTrue(ledger.isDue(song, 0L))
    }

    @Test
    fun aPublicationAbandonedLeavesItsDocumentAFreshCountOnAnotherTry() {
        val ledger = LandingLedger()
        val tries = LandingLedger.PUBLICATION_TRIES_AT_MOST

        repeat(tries) { ledger.notPublished(document, 0L, "not made available") }

        assertEquals(0, ledger.unpublishedCount)
        assertFalse(ledger.notPublished(document, 0L, "not made available"))
        assertEquals(1, ledger.unpublishedCount)
    }

    @Test
    fun aPublicationThatSucceedsOnTheSecondTryLandsTheSongOnce() {
        val ledger = LandingLedger()
        val pause = LandingLedger.PUBLICATION_PAUSE_MS

        ledger.landed(song)
        assertFalse(ledger.notPublished(document, 0L, "not made available"))
        assertEquals(listOf(document), ledger.duePublications(pause))
        ledger.published(document)

        assertEquals(0, ledger.unpublishedCount)
        assertTrue(ledger.duePublications(Long.MAX_VALUE).isEmpty())
        assertTrue(ledger.isDue(song, 0L))
        assertFalse(ledger.notPublished("content://media/external/file/43", 0L, "again"))
        assertEquals(1, ledger.unpublishedCount)
    }

    @Test
    fun aPublicationFailingAgainIsWaitedOutAgain() {
        val ledger = LandingLedger()
        val pause = LandingLedger.PUBLICATION_PAUSE_MS

        ledger.notPublished(document, 0L, "first")
        ledger.notPublished(document, pause, "second")

        assertEquals(1, ledger.unpublishedCount)
        assertTrue(ledger.duePublications(pause + 1).isEmpty())
        assertEquals(listOf(document), ledger.duePublications(pause * 2))
        assertEquals("second", ledger.failure)
    }

    @Test
    fun aPublicationWaitingKeepsTheTransferGoingWhateverTheDestination() {
        val ledger = LandingLedger()

        ledger.notPublished(document, 0L, "not made available")
        val work = ledger.work(0, LandingDestination.Away, false)

        assertTrue(work.progressing)
        assertTrue(work.transferring)
        assertEquals("not made available", work.notice(English))
    }

    @Test
    fun theFinalRefusalClearsTheNoticeAndALaterSongShowsItsOwn() {
        val ledger = LandingLedger()
        val pause = LandingLedger.LANDING_PAUSE_MS

        assertFalse(ledger.refuse(song, 0L, "first"))
        assertFalse(ledger.isDue(song, pause - 1))
        assertFalse(ledger.refuse(song, pause, "second"))
        assertTrue(ledger.refuse(song, pause * 2, "third"))

        val settled = ledger.work(0, into, false)

        assertNull(settled.notice(English))
        assertFalse(settled.transferring)

        ledger.settle(emptyList())

        assertNull(ledger.failure)
        assertNull(ledger.work(1, into, false).notice(English))
        assertTrue(ledger.work(1, into, false).transferring)
    }

    @Test
    fun aSongStillWaitingKeepsItsFailureOnShow() {
        val ledger = LandingLedger()

        ledger.refuse(song, 0L, "could not be kept")
        ledger.settle(listOf(song))

        assertEquals("could not be kept", ledger.work(1, into, false).notice(English))
    }

    @Test
    fun aSongThatLandsLeavesNoRefusalBehind() {
        val ledger = LandingLedger()

        ledger.refuse(song, 0L, "could not be kept")
        ledger.landed(song)

        assertTrue(ledger.isDue(song, 0L))
        assertFalse(ledger.refuse(song, 0L, "again"))
    }

    @Test
    fun aRefusalForASongNoLongerWaitingIsForgotten() {
        val ledger = LandingLedger()

        ledger.refuse(song, 0L, "could not be kept")
        ledger.settle(listOf(8uL))

        assertTrue(ledger.isDue(song, 0L))
    }

    @Test
    fun anUnreachableDestinationHoldsSongsWithoutHoldingTheTransfer() {
        val ledger = LandingLedger()

        val away = ledger.work(3, LandingDestination.Away, false)
        val nowhere = ledger.work(3, LandingDestination.Nowhere, false)

        assertFalse(away.transferring)
        assertFalse(nowhere.transferring)
        assertEquals(English.string(SyncLanding.AWAY), away.notice(English))
        assertEquals(English.string(SyncLanding.NOWHERE), nowhere.notice(English))
        assertEquals(3, away.pending)
    }

    @Test
    fun aDestinationBackWakesTheLandingAgain() {
        val ledger = LandingLedger()

        val back = ledger.work(3, into, false)

        assertTrue(back.transferring)
        assertNull(back.notice(English))
    }

    @Test
    fun songsMovingFromAnotherDeviceHoldTheTransferWhereverTheyWillLand() {
        val ledger = LandingLedger()

        assertTrue(ledger.work(0, LandingDestination.Away, true).transferring)
        assertFalse(ledger.work(0, into, false).transferring)
    }

    @Test
    fun theBackgroundPollIsQuickOnlyWhileSomethingCanProgress() {
        val ledger = LandingLedger()
        val away = ledger.work(2, LandingDestination.Away, false)
        val reachable = ledger.work(2, into, false)

        assertEquals(LandingWork.QUIET_POLL_MS, away.pollEveryMs(false, 0))
        assertEquals(LandingWork.QUICK_POLL_MS, reachable.pollEveryMs(false, 0))
        assertEquals(LandingWork.QUICK_POLL_MS, away.pollEveryMs(true, 0))
        assertEquals(LandingWork.QUICK_POLL_MS, away.pollEveryMs(false, 1))
    }

    @Test
    fun foldersAreReadAgainOnlyWhileSongsAreHeldBackAndNotTooOften() {
        val ledger = LandingLedger()
        val every = LandingWork.FOLDERS_REREAD_EVERY_MS
        val away = ledger.work(2, LandingDestination.Away, false)

        assertFalse(away.rereadsFolders(every - 1, 0L))
        assertTrue(away.rereadsFolders(every, 0L))
        assertFalse(ledger.work(0, LandingDestination.Away, false).rereadsFolders(every, 0L))
        assertFalse(ledger.work(2, into, false).rereadsFolders(every, 0L))
    }

    @Test
    fun aFailureSaysItsOwnLocalisedSentence() {
        assertEquals(
            English.string(R.string.features_sync_landing_not_recorded),
            LandingLedger.said(English, LandingFailed(R.string.features_sync_landing_not_recorded)),
        )
        assertEquals(
            English.string(R.string.features_sync_landing_not_kept),
            LandingLedger.said(English, java.io.IOException("No space left on device")),
        )
    }
}
