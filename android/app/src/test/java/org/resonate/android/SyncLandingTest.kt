package org.resonate.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.resonate.android.nativecore.SyncStanding
import org.resonate.android.nativecore.SyncStatus

class SyncLandingTest {
    private val music = LandingFolder("music", readable = true, writable = true, downloads = false)
    private val card = LandingFolder("card", readable = false, writable = true, downloads = false)
    private val downloads =
        LandingFolder("downloads", readable = true, writable = true, downloads = true)

    @Test
    fun aFolderOutOfReachHoldsTheSongsBackRatherThanSendingThemElsewhere() {
        assertEquals(
            LandingDestination.Away,
            SyncLanding.destination(listOf(card, music), "card"),
        )
        assertEquals(LandingDestination.Away, SyncLanding.destination(listOf(card, music), null))
    }

    @Test
    fun theChosenFolderIsTakenElseTheFirstWritableOneOtherThanDownloads() {
        assertEquals(
            LandingDestination.Into("downloads"),
            SyncLanding.destination(listOf(downloads, music), "downloads"),
        )
        assertEquals(
            LandingDestination.Into("music"),
            SyncLanding.destination(listOf(downloads, music), null),
        )
        assertEquals(
            LandingDestination.Into("music"),
            SyncLanding.destination(listOf(downloads, music), "gone"),
        )
    }

    @Test
    fun noWritableFolderLeavesNowhereToLand() {
        assertEquals(
            LandingDestination.Nowhere,
            SyncLanding.destination(listOf(downloads, music.copy(writable = false)), null),
        )
    }

    @Test
    fun choosingDownloadsOpensTheLandingTheSongsWereHeldBackFrom() {
        val before = SyncLanding.destination(listOf(downloads), null)
        val after = SyncLanding.destination(listOf(downloads), "downloads")

        assertEquals(LandingDestination.Nowhere, before)
        assertTrue(SyncLanding.opened(before, after))
        assertTrue(SyncLanding.opened(LandingDestination.Away, after))
    }

    @Test
    fun aLandingNeverHeldBackOrStillHeldBackOpensNothing() {
        val into = LandingDestination.Into("music")

        assertFalse(SyncLanding.opened(null, into))
        assertFalse(SyncLanding.opened(into, into))
        assertFalse(SyncLanding.opened(LandingDestination.Nowhere, LandingDestination.Away))
    }

    private fun status(
        standing: SyncStanding = SyncStanding.CONNECTED,
        moving: UInt = 0u,
        settled: Boolean = true,
    ) = SyncStatus(standing, "", emptyList(), moving, 0u, 0u, 0u, 0uL, settled, emptyList())

    @Test
    fun aSyncRunningIsJoinedRatherThanAskedAgain() {
        assertTrue(SyncLanding.followsAgain(status()))
        assertFalse(SyncLanding.followsAgain(status(moving = 1u)))
        assertFalse(SyncLanding.followsAgain(status(settled = false)))
        assertFalse(SyncLanding.followsAgain(status(SyncStanding.OFF)))
        assertFalse(SyncLanding.followsAgain(null))
    }

    @Test
    fun theFetchBeginningIsCountedAndNamesTheFolder() {
        assertEquals(
            "Fetching 1 song from your devices into Downloads",
            SyncLanding.beginning(English, 1, "Downloads"),
        )
        assertEquals(
            "Fetching 12 songs from your devices into Music",
            SyncLanding.beginning(English, 12, "Music"),
        )
        assertEquals(
            "Asking your devices for their songs, kept in Music",
            SyncLanding.beginning(English, 0, "Music"),
        )
        assertEquals("Keeping 3 songs from your devices", SyncLanding.keeping(English, 3))
    }
}
