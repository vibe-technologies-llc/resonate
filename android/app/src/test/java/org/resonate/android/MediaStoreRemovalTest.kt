package org.resonate.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class MediaStoreRemovalTest {
    @Test
    fun aQueryThatFindsARowIsStandingAndOneThatFindsNoneIsAbsent() {
        assertEquals(RowPresence.Standing, MediaStoreRemoval.presence { true })
        assertEquals(RowPresence.Absent, MediaStoreRemoval.presence { false })
    }

    @Test
    fun aNullCursorIsUnknownNeverAbsent() {
        assertEquals(RowPresence.Unknown, MediaStoreRemoval.presence { null })
    }

    @Test
    fun aDeniedQueryIsUnknownNeverAbsent() {
        assertEquals(
            RowPresence.Unknown,
            MediaStoreRemoval.presence { throw SecurityException("denied") },
        )
    }

    @Test
    fun aDeleteThatRemovedARowIsSettledWithoutAskingAgain() {
        assertTrue(MediaStoreRemoval.settled(1) { error("asked again") })
    }

    @Test
    fun aDeleteThatRemovedNothingIsSettledOnlyWhereTheRowIsConfirmedAbsent() {
        assertTrue(MediaStoreRemoval.settled(0) { RowPresence.Absent })
        assertFalse(MediaStoreRemoval.settled(0) { RowPresence.Standing })
        assertFalse(MediaStoreRemoval.settled(0) { RowPresence.Unknown })
    }
}
