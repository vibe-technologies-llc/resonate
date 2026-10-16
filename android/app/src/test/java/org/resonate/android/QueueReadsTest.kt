package org.resonate.android

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class QueueReadsTest {
    private val reads = QueueReads(catalogRereadAfterMs = 3000)

    @Test
    fun theFirstTickReadsTheQueue() {
        assertTrue(reads.due(1uL, 1uL, 0))
    }

    @Test
    fun nothingMovedReadsNothing() {
        reads.noted(4uL, 9uL, 0)

        assertFalse(reads.due(4uL, 9uL, 250))
        assertFalse(reads.due(4uL, 9uL, 60_000))
    }

    @Test
    fun aMovedQueueRevisionReadsAtOnce() {
        reads.noted(4uL, 9uL, 0)

        assertTrue(reads.due(5uL, 9uL, 250))
    }

    @Test
    fun aMovedCatalogRevisionReadsOnlyOnceThreeSecondsHavePassed() {
        reads.noted(4uL, 9uL, 1000)

        assertFalse(reads.due(4uL, 10uL, 2999))
        assertTrue(reads.due(4uL, 10uL, 4000))
    }

    @Test
    fun aReadNotedStartsTheWaitAgain() {
        reads.noted(4uL, 9uL, 0)
        reads.noted(4uL, 10uL, 5000)

        assertFalse(reads.due(4uL, 10uL, 9000))
        assertFalse(reads.due(4uL, 11uL, 6000))
        assertTrue(reads.due(4uL, 11uL, 8000))
    }
}
