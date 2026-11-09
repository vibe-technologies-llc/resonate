package org.resonate.android.ui.settings

import java.util.Locale
import org.resonate.android.R
import org.resonate.android.Words
import org.resonate.android.nativecore.SyncConfig
import org.resonate.android.nativecore.SyncStanding
import org.resonate.android.nativecore.SyncStatus
import org.resonate.android.nativecore.SyncTransfer
import org.resonate.android.nativecore.SyncTransferState
import org.resonate.android.nativecore.SyncedDevice

private fun SyncStatus?.others(): List<SyncedDevice> = this?.devices.orEmpty().filter { !it.thisDevice }

fun syncHeadline(words: Words, status: SyncStatus?, config: SyncConfig, online: Boolean): String {
    if (!config.enabled) return words.string(R.string.sync_headline_off)
    if (!online && !config.nearbyOnly) return words.string(R.string.sync_headline_waiting_online)
    val nearby = status.others().any { it.nearby }
    return when (status?.standing) {
        null -> words.string(R.string.sync_starting)
        SyncStanding.OFF -> words.string(R.string.sync_headline_off)
        SyncStanding.CONNECTING -> words.string(R.string.sync_headline_connecting)
        SyncStanding.REFUSED -> words.string(R.string.sync_headline_refused)
        SyncStanding.UNREACHABLE ->
            words.string(
                if (nearby) R.string.sync_headline_catching_up
                else R.string.sync_headline_not_syncing
            )
        SyncStanding.NEARBY ->
            words.string(
                if (nearby) R.string.sync_headline_catching_up else R.string.sync_headline_looking
            )
        SyncStanding.CONNECTED ->
            if (status.owed > 0u) {
                val owed = status.owed.toInt()
                words.plural(R.plurals.sync_headline_sending, owed, owed)
            } else {
                words.string(R.string.sync_headline_up_to_date)
            }
    }
}

fun syncStanding(words: Words, status: SyncStatus?, config: SyncConfig, online: Boolean): String {
    if (!config.enabled) return words.string(R.string.sync_standing_off)
    if (!online && !config.nearbyOnly) return words.string(R.string.sync_standing_offline)
    val others = status.others()
    val nearby = others.count { it.nearby }
    val reached = others.count { it.online }
    return when (status?.standing) {
        SyncStanding.CONNECTED ->
            if (reached == 0) words.string(R.string.sync_standing_none_online)
            else words.plural(R.plurals.sync_standing_online, reached, reached)
        SyncStanding.NEARBY ->
            if (nearby == 0) words.string(R.string.sync_standing_looking_nearby)
            else words.plural(R.plurals.sync_standing_nearby, nearby, nearby)
        SyncStanding.UNREACHABLE ->
            if (nearby == 0) words.string(R.string.sync_standing_unreachable)
            else words.string(R.string.sync_standing_unreachable_nearby, nearby)
        SyncStanding.REFUSED -> words.string(R.string.sync_standing_refused)
        SyncStanding.CONNECTING ->
            if (nearby == 0) words.string(R.string.sync_standing_reaching)
            else words.string(R.string.sync_standing_reaching_nearby, nearby)
        SyncStanding.OFF,
        null -> words.string(R.string.sync_starting)
    }
}

fun orderedDevices(devices: List<SyncedDevice>): List<SyncedDevice> =
    devices.sortedWith(
        compareByDescending<SyncedDevice> { it.thisDevice }
            .thenByDescending { it.online }
            .thenByDescending { it.nearby }
            .thenBy { it.name.lowercase(Locale.ROOT) }
    )

fun deviceState(words: Words, device: SyncedDevice): String =
    words.string(
        when {
            device.thisDevice -> R.string.sync_device_this
            device.online && device.nearby -> R.string.sync_device_online_nearby
            device.online -> R.string.sync_device_online
            device.nearby -> R.string.sync_device_nearby
            else -> R.string.sync_device_offline
        }
    )

fun mayForget(device: SyncedDevice): Boolean = !device.thisDevice && !device.online

private fun megabytes(bytes: ULong): String =
    String.format(Locale.getDefault(), "%.1f", bytes.toDouble() / 1_048_576.0)

fun transferLine(words: Words, transfer: SyncTransfer): String =
    listOfNotNull(
            transfer.artist.takeIf { it.isNotBlank() },
            transfer.from
                .takeIf { it.isNotBlank() }
                ?.let { words.string(R.string.sync_transfer_from, it) },
            when (transfer.state) {
                SyncTransferState.WAITING -> words.string(R.string.sync_transfer_waiting)
                SyncTransferState.FAILED -> words.string(R.string.sync_transfer_failed)
                SyncTransferState.LANDED -> words.string(R.string.sync_transfer_kept)
                SyncTransferState.MOVING ->
                    if (transfer.size > 0uL)
                        words.string(
                            R.string.sync_transfer_megabytes,
                            megabytes(minOf(transfer.received, transfer.size)),
                            megabytes(transfer.size),
                        )
                    else null
            },
        )
        .joinToString(" · ")

fun transfersHeader(words: Words, downloading: UInt, waiting: UInt): String =
    listOfNotNull(
            words.string(R.string.sync_transfers),
            words
                .plural(R.plurals.sync_transfers_downloading, downloading.toInt(), downloading.toInt())
                .takeIf { downloading > 0u },
            words
                .plural(R.plurals.sync_transfers_waiting, waiting.toInt(), waiting.toInt())
                .takeIf { waiting > 0u },
        )
        .joinToString(" · ")

fun transferProgress(transfer: SyncTransfer): Float? =
    if (transfer.size == 0uL) null
    else (minOf(transfer.received, transfer.size).toDouble() / transfer.size.toDouble()).toFloat()

fun namesUnsaved(stored: SyncConfig, device: String, server: String): Boolean =
    device.trim() != stored.device || server.trim() != stored.server

fun codeLines(code: String, groupsPerLine: Int = 3): String =
    code.split('-').chunked(groupsPerLine).joinToString("-\n") { it.joinToString("-") }
