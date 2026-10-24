package org.resonate.android.ui

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import org.resonate.android.nativecore.SyncStanding
import org.resonate.android.nativecore.SyncStatus
import org.resonate.android.nativecore.SyncTransfer
import org.resonate.android.nativecore.SyncTransferState

class SyncActivityTest {
    private fun status(
        standing: SyncStanding = SyncStanding.CONNECTED,
        moving: UInt = 0u,
        settled: Boolean = true,
        transfers: List<SyncTransfer> = emptyList(),
    ) = SyncStatus(standing, "", emptyList(), moving, 0u, 0u, 0u, 0uL, settled, transfers)

    private fun transfer(state: SyncTransferState, received: ULong, size: ULong) =
        SyncTransfer("Song", "Artist", "Phone", received, size, state)

    @Test
    fun syncSwitchedOffShowsNothing() {
        assertNull(syncActivity(status(SyncStanding.OFF, moving = 3u), 0L, 0L))
        assertNull(syncActivity(null, 0L, 0L))
    }

    @Test
    fun songsOnTheirWayAreFetchingWithTheShareReceived() {
        val activity =
            syncActivity(
                status(
                    transfers =
                        listOf(
                            transfer(SyncTransferState.MOVING, 50uL, 100uL),
                            transfer(SyncTransferState.WAITING, 0uL, 100uL),
                            transfer(SyncTransferState.LANDED, 100uL, 100uL),
                        )
                ),
                null,
                0L,
            )

        assertEquals(SyncActivity.Fetching(2, 0.25f), activity)
    }

    @Test
    fun aTransferOfUnknownSizeLeavesTheShareUnknown() {
        val activity =
            syncActivity(
                status(
                    transfers =
                        listOf(
                            transfer(SyncTransferState.MOVING, 50uL, 100uL),
                            transfer(SyncTransferState.MOVING, 10uL, 0uL),
                        )
                ),
                null,
                0L,
            )

        assertEquals(SyncActivity.Fetching(2, null), activity)
    }

    @Test
    fun connectingOrMovingIsSyncing() {
        assertEquals(SyncActivity.Syncing, syncActivity(status(SyncStanding.CONNECTING), null, 0L))
        assertEquals(SyncActivity.Syncing, syncActivity(status(moving = 1u), null, 0L))
    }

    @Test
    fun anAskedSyncShowsUntilSettledOrItsTimeHasPassed() {
        assertEquals(SyncActivity.Syncing, syncActivity(status(settled = false), 1_000L, 2_000L))
        assertNull(syncActivity(status(settled = true), 1_000L, 2_000L))
        assertNull(
            syncActivity(status(settled = false), 1_000L, 1_000L + SYNC_ASKED_SHOWS_FOR_MS)
        )
    }
}
