package org.resonate.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.resonate.android.nativecore.DevicePlatform
import org.resonate.android.nativecore.SyncStanding
import org.resonate.android.nativecore.SyncStatus
import org.resonate.android.nativecore.SyncedDevice

class BackgroundSyncTest {
    private fun device(online: Boolean = false, nearby: Boolean = false, here: Boolean = false) =
        SyncedDevice("00", "Desk", DevicePlatform.LINUX, online, nearby, here)

    private fun status(
        standing: SyncStanding = SyncStanding.CONNECTED,
        moving: UInt = 0u,
        settled: Boolean = true,
        devices: List<SyncedDevice> = emptyList(),
    ) = SyncStatus(standing, "", devices, moving, 0u, 0u, 0u, 0uL, settled, emptyList())

    private val quietLongEnough = BackgroundSync.QUIET_FOR_MS
    private val heardLongEnough = BackgroundSync.HEARD_FOR_AT_LEAST_MS

    @Test
    fun aSettledSyncWithNothingMovingIsQuiet() {
        assertTrue(BackgroundSync.quiet(status()))
    }

    @Test
    fun songsOnTheirWayAreNeverQuiet() {
        assertFalse(BackgroundSync.quiet(status(moving = 1u)))
        assertFalse(BackgroundSync.quiet(status(SyncStanding.UNREACHABLE, moving = 2u)))
    }

    @Test
    fun anUnsettledSyncWaitsWhileItReachesTheServerOrADevice() {
        assertFalse(BackgroundSync.quiet(status(SyncStanding.CONNECTING, settled = false)))
        assertFalse(BackgroundSync.quiet(status(SyncStanding.CONNECTED, settled = false)))
        assertFalse(
            BackgroundSync.quiet(
                status(SyncStanding.NEARBY, settled = false, devices = listOf(device(nearby = true)))
            )
        )
    }

    @Test
    fun anUnsettledSyncWithNobodyToMeetIsQuiet() {
        val alone = listOf(device(online = true, here = true), device())

        assertTrue(BackgroundSync.quiet(status(SyncStanding.UNREACHABLE, settled = false, devices = alone)))
        assertTrue(BackgroundSync.quiet(status(SyncStanding.NEARBY, settled = false, devices = alone)))
        assertTrue(BackgroundSync.quiet(status(SyncStanding.REFUSED, settled = false)))
    }

    @Test
    fun syncSwitchedOffEndsThePassAtOnce() {
        assertEquals(BackgroundPass.DONE, BackgroundSync.verdict(status(SyncStanding.OFF), 0L, null))
    }

    @Test
    fun aQuietSyncEndsThePassOnlyOnceHeardAndQuietLongEnough() {
        val quiet = status()

        assertEquals(BackgroundPass.GOING, BackgroundSync.verdict(quiet, heardLongEnough - 1, quietLongEnough))
        assertEquals(BackgroundPass.GOING, BackgroundSync.verdict(quiet, heardLongEnough, quietLongEnough - 1))
        assertEquals(BackgroundPass.DONE, BackgroundSync.verdict(quiet, heardLongEnough, quietLongEnough))
    }

    @Test
    fun aPassStillBusyRunsOutOfTime() {
        val busy = status(moving = 3u)

        assertEquals(
            BackgroundPass.GOING,
            BackgroundSync.verdict(busy, BackgroundSync.RUNS_AT_MOST_MS - 1, null),
        )
        assertEquals(
            BackgroundPass.OUT_OF_TIME,
            BackgroundSync.verdict(busy, BackgroundSync.RUNS_AT_MOST_MS, null),
        )
    }
}
