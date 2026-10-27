package org.resonate.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class FindingNoticesTest {
    @Test
    fun whatTheLibraryFindsByItselfIsSilentUnlessTheListenerAsksToHearIt() {
        assertNull(FindingNotices.shown(false, FindingNotices.kept(20)))
        assertEquals(
            "Kept 20 songs from your devices",
            FindingNotices.shown(true, FindingNotices.kept(20)),
        )
    }

    @Test
    fun keptSongsAreCounted() {
        assertEquals("Kept 1 song from your devices", FindingNotices.kept(1))
        assertEquals("Kept 3 songs from your devices", FindingNotices.kept(3))
    }

    @Test
    fun hiddenSongsAreCountedAndSayWhenTheDevicesAreAsked() {
        assertEquals("1 song whose file is gone was hidden", FindingNotices.hidden(1u, false))
        assertEquals(
            "4 songs whose files are gone were hidden · fetching from your devices",
            FindingNotices.hidden(4u, true),
        )
    }
}
