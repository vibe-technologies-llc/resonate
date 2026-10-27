package org.resonate.android

import org.junit.Assert.assertEquals
import org.junit.Test

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
}
