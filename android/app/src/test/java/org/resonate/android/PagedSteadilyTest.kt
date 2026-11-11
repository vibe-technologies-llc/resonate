package org.resonate.android

import org.junit.Assert.assertEquals
import org.junit.Test

class PagedSteadilyTest {
    private class MovingCatalog(var rows: List<Int>) {
        var revision = 0uL
        var pagesRead = 0
        var arrivesAfterPage: Int? = null
        var arriving = 0

        fun page(offset: UInt, limit: UInt): List<Int> {
            val read = rows.drop(offset.toInt()).take(limit.toInt())
            pagesRead++
            if (pagesRead == arrivesAfterPage) {
                rows = listOf(arriving) + rows
                revision++
            }
            return read
        }
    }

    @Test
    fun aSongArrivingBetweenPagesStartsTheWalkAgainAndNoRowIsReadTwice() {
        val catalog = MovingCatalog((1..10).toList())
        catalog.arrivesAfterPage = 1
        catalog.arriving = 0

        val read = pagedSteadily(4u, { catalog.revision }, { it }, { true }, catalog::page)

        assertEquals((0..10).toList(), read)
    }

    @Test
    fun aCatalogThatNeverStopsMovingIsStillReadOnceForEachRow() {
        val catalog = MovingCatalog((1..10).toList())
        var next = -1
        val read =
            pagedSteadily(4u, { catalog.revision }, { it }, { true }) { offset, limit ->
                val page = catalog.page(offset, limit)
                catalog.rows = listOf(next--) + catalog.rows
                catalog.revision++
                page
            }

        assertEquals(read.distinct(), read)
    }

    @Test
    fun aStillCatalogIsReadOnceInPages() {
        val catalog = MovingCatalog((1..9).toList())

        val read = pagedSteadily(4u, { catalog.revision }, { it }, { true }, catalog::page)

        assertEquals((1..9).toList(), read)
        assertEquals(3, catalog.pagesRead)
    }

    private class PinnedCatalog(var rows: List<Int>) {
        var revision = 0uL
        val kept = mutableMapOf(0uL to rows)
        var pagesRead = 0
        var pinsKeptAtMost = Int.MAX_VALUE

        fun arrive(row: Int) {
            rows = listOf(row) + rows
            revision++
            kept[revision] = rows
            while (kept.size > pinsKeptAtMost) kept.remove(kept.keys.min())
        }

        fun page(offset: UInt, limit: UInt, pinned: ULong?): Pair<List<Int>, ULong> {
            pagesRead++
            val read = pinned?.takeIf { it in kept } ?: revision
            return kept.getValue(read).drop(offset.toInt()).take(limit.toInt()) to read
        }
    }

    @Test
    fun aWalkPinnedToItsFirstPageReadsOnceThoughSongsArriveBetweenPages() {
        val catalog = PinnedCatalog((1..10).toList())
        var arriving = 0

        val read =
            pagedPinned(4u, { it }, { true }) { offset, limit, pinned ->
                catalog.page(offset, limit, pinned).also { catalog.arrive(arriving--) }
            }

        assertEquals((1..10).toList(), read)
        assertEquals(3, catalog.pagesRead)
    }

    @Test
    fun aWalkWhosePinWasLetGoStartsAgainOnTheCatalogAsItStands() {
        val catalog = PinnedCatalog((1..10).toList())
        catalog.pinsKeptAtMost = 1
        var arrivals = 1

        val read =
            pagedPinned(4u, { it }, { true }) { offset, limit, pinned ->
                catalog.page(offset, limit, pinned).also {
                    if (arrivals-- > 0) catalog.arrive(0)
                }
            }

        assertEquals((0..10).toList(), read)
        assertEquals(5, catalog.pagesRead)
    }
}
