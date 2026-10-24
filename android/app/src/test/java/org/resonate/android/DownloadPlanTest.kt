package org.resonate.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.resonate.android.nativecore.SearchSong

class DownloadPlanTest {
    private fun song(title: String, recording: String = title) =
        SearchSong(recording, title, "Artist", "Album", null, emptyList())

    private fun job(
        key: Long,
        title: String,
        state: DownloadState = DownloadState.WAITING,
        received: ULong = 0uL,
        failure: String? = null,
    ) = DownloadJob(key, song(title), "folder", state, null, received, null, failure)

    @Test
    fun twoJobsRunAtOnceAndTheRestWaitInOrder() {
        val jobs =
            listOf(
                job(1, "a", DownloadState.DOWNLOADING),
                job(2, "b"),
                job(3, "c"),
                job(4, "d"),
            )

        assertEquals(listOf(2L), DownloadPlan.startable(jobs))
        assertEquals(listOf(1L, 2L), DownloadPlan.startable(listOf(job(1, "a"), job(2, "b"), job(3, "c"))))
        assertTrue(DownloadPlan.startable(jobs.map { it.copy(state = DownloadState.FINDING) }).isEmpty())
    }

    @Test
    fun aSettledJobHoldsNothingAndAnActiveOneHoldsItsSong() {
        val jobs = listOf(job(1, "a", DownloadState.DONE), job(2, "b", DownloadState.FINDING))

        assertEquals(setOf("b"), DownloadPlan.held(jobs))
    }

    @Test
    fun aSongWithNoRecordingIsKeptByTitleAndArtist() {
        assertEquals("Title\u001fArtist", song("Title", recording = "").heldKey())
    }

    @Test
    fun aLoneJobSaysWhatItIsDoingAndHowItEnded() {
        assertNull(DownloadPlan.summary(emptyList()))
        assertEquals("Finding a…", DownloadPlan.summary(listOf(job(1, "a", DownloadState.FINDING))))
        assertEquals("Downloading a…", DownloadPlan.summary(listOf(job(1, "a", DownloadState.DOWNLOADING))))
        assertEquals(
            "Downloaded a · 2 KiB",
            DownloadPlan.summary(listOf(job(1, "a", DownloadState.DONE, 2048uL))),
        )
        assertEquals(
            "No provider has it",
            DownloadPlan.summary(listOf(job(1, "a", DownloadState.FAILED, failure = "No provider has it"))),
        )
        assertEquals(
            "Download cancelled",
            DownloadPlan.summary(listOf(job(1, "a", DownloadState.CANCELLED))),
        )
    }

    @Test
    fun aBatchCountsWhereItIsAndWhatFailed() {
        val running =
            listOf(job(1, "a", DownloadState.DONE), job(2, "b", DownloadState.DOWNLOADING), job(3, "c"))

        assertEquals("Downloading 2 of 3 · Downloading b…", DownloadPlan.summary(running))
        assertEquals(
            "Downloaded 1 of 2 songs · 1 failed: Gone",
            DownloadPlan.summary(
                listOf(job(1, "a", DownloadState.DONE), job(2, "b", DownloadState.FAILED, failure = "Gone"))
            ),
        )
        assertEquals(
            "Downloaded 2 of 2 songs",
            DownloadPlan.summary(listOf(job(1, "a", DownloadState.DONE), job(2, "b", DownloadState.DONE))),
        )
    }

    @Test
    fun receivedBytesCountOnlyWhatIsInFlight() {
        val jobs =
            listOf(
                job(1, "a", DownloadState.DOWNLOADING, 100uL),
                job(2, "b", DownloadState.DONE, 900uL),
                job(3, "c", DownloadState.FINDING, 5uL),
            )

        assertEquals(105uL, DownloadPlan.received(jobs))
    }

    @Test
    fun theWantsAreAskedNoSoonerThanThirtySecondsAndNoLaterThanHalfAnHour() {
        assertEquals(30_000L, DownloadPlan.waitForWants(100, 100))
        assertEquals(30_000L, DownloadPlan.waitForWants(50, 100))
        assertEquals(120_000L, DownloadPlan.waitForWants(220, 100))
        assertEquals(1_800_000L, DownloadPlan.waitForWants(1_000_000, 100))
    }

    @Test
    fun aFinishedBatchIsReplacedAndAnUnfinishedOneKept() {
        val finished = listOf(job(1, "a", DownloadState.DONE))
        val open = listOf(job(1, "a", DownloadState.DONE), job(2, "b"))

        assertTrue(DownloadPlan.nextBatch(finished).isEmpty())
        assertEquals(open, DownloadPlan.nextBatch(open))
    }

    @Test
    fun onlyAJournalledDocumentTheCatalogDoesNotHoldIsStranded() {
        val journalled = listOf("content://a", "content://b", "content://c")

        val stranded = DownloadPlan.stranded(journalled, setOf("content://b", "content://other"))

        assertEquals(setOf("content://a", "content://c"), stranded)
        assertTrue(DownloadPlan.stranded(journalled, journalled.toSet()).isEmpty())
        assertTrue(DownloadPlan.stranded(emptyList(), setOf("content://b")).isEmpty())
    }
}
