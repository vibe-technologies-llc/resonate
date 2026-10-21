package org.resonate.android.ui

import org.resonate.android.nativecore.SyncStanding
import org.resonate.android.nativecore.SyncStatus
import org.resonate.android.nativecore.SyncTransferState

sealed interface SyncActivity {
    data object Syncing : SyncActivity

    data class Fetching(val songs: Int, val fraction: Float?) : SyncActivity
}

const val SYNC_ASKED_SHOWS_FOR_MS = 30_000L

fun syncActivity(status: SyncStatus?, askedAtMs: Long?, nowMs: Long): SyncActivity? {
    if (status == null || status.standing == SyncStanding.OFF) return null
    val active =
        status.transfers.filter {
            it.state == SyncTransferState.WAITING || it.state == SyncTransferState.MOVING
        }
    if (active.isNotEmpty()) {
        val sized = active.filter { it.size > 0u }
        val total = sized.sumOf { it.size }
        val received = sized.sumOf { minOf(it.received, it.size) }
        val fraction =
            if (total == 0uL || sized.size != active.size) null
            else (received.toDouble() / total.toDouble()).toFloat().coerceIn(0f, 1f)
        return SyncActivity.Fetching(active.size, fraction)
    }
    val recentlyAsked = askedAtMs != null && nowMs - askedAtMs < SYNC_ASKED_SHOWS_FOR_MS
    return if (
        (recentlyAsked && !status.settled) ||
            status.standing == SyncStanding.CONNECTING ||
            status.moving > 0u
    )
        SyncActivity.Syncing
    else null
}
