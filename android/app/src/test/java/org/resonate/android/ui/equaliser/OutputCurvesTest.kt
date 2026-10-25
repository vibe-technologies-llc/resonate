package org.resonate.android.ui.equaliser

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.resonate.android.OutputRoute

class OutputCurvesTest {
    private val buds = OutputRoute("v1:bluetooth:AA", "Buds")
    private val car = OutputRoute("v1:bluetooth:BB", "Car")

    @Test
    fun anOutputIsBoundOnlyWhereItsIdIsKept() {
        assertTrue(isBound(buds, listOf(car, buds)))
        assertFalse(isBound(buds, listOf(car)))
        assertFalse(isBound(null, listOf(car, buds)))
    }

    @Test
    fun theOtherOutputsAreEveryKeptCurveButTheOneInUse() {
        assertEquals(listOf(car), keptForOthers(buds, listOf(car, buds)))
        assertEquals(listOf(car, buds), keptForOthers(null, listOf(car, buds)))
    }

    @Test
    fun theCurveIsSaidToBelongToTheOutputThatKeepsItAndElseToEveryOutput() {
        assertEquals("Buds", curveScope(buds, listOf(buds)))
        assertEquals("Every output", curveScope(buds, listOf(car)))
        assertEquals("Every output", curveScope(null, listOf(car)))
    }
}
