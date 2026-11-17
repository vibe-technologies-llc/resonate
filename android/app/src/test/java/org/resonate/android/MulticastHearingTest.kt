package org.resonate.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class MulticastHearingTest {
    private class Lock : HeldLock {
        override var isHeld = false
        var acquired = 0

        override fun acquire() {
            isHeld = true
            acquired++
        }

        override fun release() {
            isHeld = false
        }
    }

    private val lock = Lock()
    private val hearing = MulticastHearing<Any> { lock }

    @Test
    fun theEngineBegunHoldsTheLockWhileItWantsToHearAndOnlyOnce() {
        val engine = Any()

        hearing.begin(engine)
        hearing.hear(engine, true)
        hearing.hear(engine, true)
        val heard = lock.isHeld
        hearing.hear(engine, false)

        assertTrue(heard)
        assertEquals(1, lock.acquired)
        assertFalse(lock.isHeld)
    }

    @Test
    fun aSettleThatEndsAfterStopTakesTheLockNoMore() {
        val engine = Any()

        hearing.begin(engine)
        hearing.hear(engine, true)
        hearing.stop()
        hearing.hear(engine, true)

        assertFalse(lock.isHeld)
        assertEquals(1, lock.acquired)
    }

    @Test
    fun anEngineClosedEarlierMovesNothingTheEngineOpenSinceSet() {
        val closed = Any()
        val open = Any()

        hearing.begin(closed)
        hearing.stop()
        hearing.begin(open)
        hearing.hear(open, true)
        hearing.hear(closed, false)

        assertTrue(lock.isHeld)

        hearing.stop()
        hearing.begin(open)
        hearing.hear(closed, true)

        assertFalse(lock.isHeld)
    }
}
