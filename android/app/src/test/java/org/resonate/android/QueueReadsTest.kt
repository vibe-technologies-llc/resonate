package org.resonate.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class QueueReadsTest {
    private val reads = QueueReads(catalogRereadAfterMs = 3000)
    private val shown = QueueShown()

    private fun asked(zone: UInt = 1u, queue: ULong = 4uL, catalog: ULong = 9uL, playing: UInt = 0u) =
        QueueAsk(zone, queue, catalog, playing)

    @Test
    fun theFirstTickReadsTheQueue() {
        assertTrue(reads.due(asked(), shown, 0))
    }

    @Test
    fun nothingMovedReadsNothing() {
        reads.noted(asked(), shown, 0)

        assertFalse(reads.due(asked(), shown, 250))
        assertFalse(reads.due(asked(), shown, 60_000))
    }

    @Test
    fun aMovedQueueRevisionReadsAtOnce() {
        reads.noted(asked(), shown, 0)

        assertTrue(reads.due(asked(queue = 5uL), shown, 250))
    }

    @Test
    fun aMovedCatalogRevisionReadsOnlyOnceThreeSecondsHavePassed() {
        reads.noted(asked(), shown, 1000)

        assertFalse(reads.due(asked(catalog = 10uL), shown, 2999))
        assertTrue(reads.due(asked(catalog = 10uL), shown, 4000))
    }

    @Test
    fun anotherZoneShownReadsItsQueueAtOnceThoughItsRevisionIsTheSame() {
        reads.noted(asked(), shown, 0)

        assertTrue(reads.due(asked(zone = 2u), shown, 250))
    }

    @Test
    fun anotherRowPlayingOrAWiderWindowReadsTheWindowAroundItAtOnce() {
        reads.noted(asked(), shown, 0)

        assertTrue(reads.due(asked(playing = 1u), shown, 250))
        assertTrue(reads.due(asked(), shown.widened(earlier = false), 250))
    }

    @Test
    fun aReadNotedStartsTheWaitAgain() {
        reads.noted(asked(), shown, 0)
        reads.noted(asked(catalog = 10uL), shown, 5000)

        assertFalse(reads.due(asked(catalog = 10uL), shown, 9000))
        assertFalse(reads.due(asked(catalog = 11uL), shown, 6000))
        assertTrue(reads.due(asked(catalog = 11uL), shown, 8000))
    }

    @Test
    fun aWindowWidensTwoHundredRowsAtTheEndAskedFor() {
        assertEquals(QueueShown(250, 200), shown.widened(earlier = true))
        assertEquals(QueueShown(50, 400), shown.widened(earlier = false))
    }
}
