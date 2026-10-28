package org.resonate.android.ui.components

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class SelectionTest {
    @Test
    fun beginningSelectsTheRowAndStartsSelecting() {
        val selection = Selection<Int>()

        assertFalse(selection.active)

        selection.begin(3)

        assertTrue(selection.active)
        assertEquals(1, selection.count)
        assertTrue(3 in selection)
    }

    @Test
    fun togglingTheLastSelectedRowLeavesSelection() {
        val selection = Selection<Int>()

        selection.begin(1)
        selection.toggle(2)
        selection.toggle(1)

        assertEquals(setOf(2), selection.chosen)

        selection.toggle(2)

        assertFalse(selection.active)
    }

    @Test
    fun aLongPressWhileSelectingAddsWithoutDroppingTheRest() {
        val selection = Selection<Int>()

        selection.begin(1)
        selection.begin(4)

        assertEquals(setOf(1, 4), selection.chosen)
    }

    @Test
    fun pickedKeepsTheOrderTheRowsAppearIn() {
        val selection = Selection<Int>()
        val rows = listOf("a" to 10, "b" to 20, "c" to 30, "d" to 40)

        selection.begin(40)
        selection.toggle(10)
        selection.toggle(30)

        assertEquals(listOf("a", "c", "d"), selection.picked(rows) { it.second }.map { it.first })
    }

    @Test
    fun selectAllTakesEveryKeyAndEveryOfSeesIt() {
        val selection = Selection<Int>()
        val keys = listOf(1, 2, 3)

        assertFalse(selection.everyOf(keys))

        selection.selectAll(keys)

        assertEquals(3, selection.count)
        assertTrue(selection.everyOf(keys))
        assertFalse(selection.everyOf(emptyList()))
    }

    @Test
    fun retainDropsRowsThatLeftTheList() {
        val selection = Selection<Int>()

        selection.selectAll(listOf(1, 2, 3))
        selection.retain(listOf(2, 3, 4))

        assertEquals(setOf(2, 3), selection.chosen)

        selection.retain(listOf(9))

        assertFalse(selection.active)
    }

    @Test
    fun clearLeavesSelection() {
        val selection = Selection<Int>()

        selection.selectAll(listOf(1, 2))
        selection.clear()

        assertFalse(selection.active)
        assertEquals(0, selection.count)
    }

    @Test
    fun aRowReportsTheStateItIsDrawnWith() {
        val selection = Selection<Int>()

        assertFalse(selection.row(1).selecting)

        selection.row(1).begin()

        assertTrue(selection.row(1).selected)
        assertTrue(selection.row(2).selecting)
        assertFalse(selection.row(2).selected)

        selection.row(2).toggle()

        assertTrue(selection.row(2).selected)
    }
}
