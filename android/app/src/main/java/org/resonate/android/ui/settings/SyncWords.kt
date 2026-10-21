package org.resonate.android.ui.settings

import java.util.Locale
import org.resonate.android.nativecore.SyncConfig
import org.resonate.android.nativecore.SyncStanding
import org.resonate.android.nativecore.SyncStatus
import org.resonate.android.nativecore.SyncTransfer
import org.resonate.android.nativecore.SyncTransferState
import org.resonate.android.nativecore.SyncedDevice
import org.resonate.android.ui.counted

private fun SyncStatus?.others(): List<SyncedDevice> = this?.devices.orEmpty().filter { !it.thisDevice }

fun syncHeadline(status: SyncStatus?, config: SyncConfig, online: Boolean): String {
    if (!config.enabled) return "Off"
    if (!online && !config.nearbyOnly) return "Waiting for online features"
    val nearby = status.others().any { it.nearby }
    return when (status?.standing) {
        null -> "Starting…"
        SyncStanding.OFF -> "Off"
        SyncStanding.CONNECTING -> "Connecting…"
        SyncStanding.REFUSED -> "Refused"
        SyncStanding.UNREACHABLE -> if (nearby) "Catching up" else "Not syncing"
        SyncStanding.NEARBY -> if (nearby) "Catching up" else "Looking for devices"
        SyncStanding.CONNECTED ->
            if (status.owed > 0u) "Sending ${counted(status.owed, "change")}" else "Up to date"
    }
}

fun syncStanding(status: SyncStatus?, config: SyncConfig, online: Boolean): String {
    if (!config.enabled) return "Playlists, favourites and history follow you across devices"
    if (!online && !config.nearbyOnly) return "Turn on online features, or sync only on this network"
    val others = status.others()
    val nearby = others.count { it.nearby }
    val reached = others.count { it.online }
    return when (status?.standing) {
        SyncStanding.CONNECTED ->
            if (reached == 0) "No other device online" else "${counted(reached, "device")} online"
        SyncStanding.NEARBY ->
            if (nearby == 0) "Looking on this network"
            else "${counted(nearby, "device")} on this network"
        SyncStanding.UNREACHABLE ->
            if (nearby == 0) "Can't reach the server" else "Server unreachable · $nearby on this network"
        SyncStanding.REFUSED -> "The server refused this device"
        SyncStanding.CONNECTING ->
            if (nearby == 0) "Reaching the server" else "Reaching the server · $nearby on this network"
        SyncStanding.OFF,
        null -> "Starting…"
    }
}

fun orderedDevices(devices: List<SyncedDevice>): List<SyncedDevice> =
    devices.sortedWith(
        compareByDescending<SyncedDevice> { it.thisDevice }
            .thenByDescending { it.online }
            .thenByDescending { it.nearby }
            .thenBy { it.name.lowercase(Locale.ROOT) }
    )

fun deviceState(device: SyncedDevice): String =
    when {
        device.thisDevice -> "This device"
        device.online && device.nearby -> "Online · on this network"
        device.online -> "Online"
        device.nearby -> "On this network"
        else -> "Offline"
    }

fun mayForget(device: SyncedDevice): Boolean = !device.thisDevice && !device.online

private fun megabytes(bytes: ULong): String =
    String.format(Locale.getDefault(), "%.1f", bytes.toDouble() / 1_048_576.0)

fun transferLine(transfer: SyncTransfer): String =
    listOfNotNull(
            transfer.artist.takeIf { it.isNotBlank() },
            transfer.from.takeIf { it.isNotBlank() }?.let { "from $it" },
            when (transfer.state) {
                SyncTransferState.WAITING -> "Waiting"
                SyncTransferState.FAILED -> "Failed"
                SyncTransferState.LANDED -> "Kept"
                SyncTransferState.MOVING ->
                    if (transfer.size > 0uL)
                        "${megabytes(minOf(transfer.received, transfer.size))} of ${megabytes(transfer.size)} MB"
                    else null
            },
        )
        .joinToString(" · ")

fun transferProgress(transfer: SyncTransfer): Float? =
    if (transfer.size == 0uL) null
    else (minOf(transfer.received, transfer.size).toDouble() / transfer.size.toDouble()).toFloat()

fun namesUnsaved(stored: SyncConfig, device: String, server: String): Boolean =
    device.trim() != stored.device || server.trim() != stored.server

fun codeLines(code: String, groupsPerLine: Int = 3): String =
    code.split('-').chunked(groupsPerLine).joinToString("-\n") { it.joinToString("-") }
