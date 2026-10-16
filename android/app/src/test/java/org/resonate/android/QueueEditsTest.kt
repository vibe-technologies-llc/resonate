package org.resonate.android

import org.junit.Assert.assertEquals
import org.junit.Test

class QueueEditsTest {
    private fun applied(rows: List<Int>, moves: List<RowMove>): List<Int> {
        val queue = rows.toMutableList()
        for (move in moves) {
            val row = queue.removeAt(move.from)
            val landing = if (move.to > move.from) move.to - 1 else move.to
            queue.add(landing, row)
        }
        return queue
    }

    @Test
    fun aRangeMovedAnywhereLandsWhereMediaThreeSaysItDoes() {
        for (size in 0..7) {
            val rows = List(size) { it }
            for (from in 0..size) {
                for (until in from..size) {
                    for (newIndex in 0..size) {
                        val range = rows.subList(from, until)
                        val rest = rows - range.toSet()
                        val at = minOf(newIndex, rest.size)
                        val expected = rest.take(at) + range + rest.drop(at)

                        val moves = QueueEdits.rangeMoved(size, from, until, newIndex)

                        assertEquals(
                            "size $size, [$from, $until) to $newIndex",
                            expected,
                            applied(rows, moves),
                        )
                    }
                }
            }
        }
    }

    @Test
    fun aRangeLeftWhereItIsMovesNothing() {
        assertEquals(emptyList<RowMove>(), QueueEdits.rangeMoved(5, 1, 3, 1))
        assertEquals(emptyList<RowMove>(), QueueEdits.rangeMoved(5, 2, 2, 4))
    }

    @Test
    fun rowsAppendedToTheEndNeedNoMoves() {
        assertEquals(emptyList<RowMove>(), QueueEdits.appendedThenMoved(4, 4, 2))
        assertEquals(emptyList<RowMove>(), QueueEdits.appendedThenMoved(4, 9, 2))
    }

    @Test
    fun rowsAppendedThenMovedLandAtTheIndexAsked() {
        for (size in 0..6) {
            val rows = List(size) { it }
            for (count in 0..3) {
                for (index in 0..size) {
                    val appended = List(count) { size + it }
                    val expected = rows.take(index) + appended + rows.drop(index)

                    val moves = QueueEdits.appendedThenMoved(size, index, count)

                    assertEquals(
                        "size $size, $count at $index",
                        expected,
                        applied(rows + appended, moves),
                    )
                }
            }
        }
    }

    @Test
    fun aRangeToRemoveNamesTheQueueRowsInIt() {
        val ids = listOf<ULong?>(10uL, null, 12uL, 13uL)

        assertEquals(listOf(12uL, 13uL), QueueEdits.rowsIn(ids, 1, 4))
        assertEquals(listOf(10uL), QueueEdits.rowsIn(ids, 0, 2))
        assertEquals(listOf(13uL), QueueEdits.rowsIn(ids, 3, 99))
        assertEquals(emptyList<ULong>(), QueueEdits.rowsIn(ids, 3, 1))
    }
}
