package org.resonate.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.resonate.android.nativecore.NewerRelease

class NewerReleasesTest {
    private val newer = NewerRelease("99.0.0", "https://github.com/someone/resonate/releases/tag/v99.0.0")

    @Test
    fun aNewerReleaseIsShownWhileCheckingAndUntilItsVersionIsDismissed() {
        assertEquals(newer, NewerReleases.shown(checks = true, heard = newer, dismissed = null))
        assertEquals(newer, NewerReleases.shown(checks = true, heard = newer, dismissed = "98.0.0"))
        assertNull(NewerReleases.shown(checks = true, heard = newer, dismissed = "99.0.0"))
        assertNull(NewerReleases.shown(checks = false, heard = newer, dismissed = null))
        assertNull(NewerReleases.shown(checks = true, heard = null, dismissed = null))
    }

    @Test
    fun theBoundaryIsAskedOnceAtFirstAndAgainOnlyAfterSixHours() {
        val looked = 1_000_000L

        assertTrue(NewerReleases.due(null, looked))
        assertFalse(NewerReleases.due(looked, looked + NewerReleases.LOOKED_AT_AGAIN_EVERY_MS - 1))
        assertTrue(NewerReleases.due(looked, looked + NewerReleases.LOOKED_AT_AGAIN_EVERY_MS))
    }
}
