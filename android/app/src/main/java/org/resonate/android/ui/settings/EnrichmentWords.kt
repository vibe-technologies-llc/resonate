package org.resonate.android.ui.settings

import org.resonate.android.nativecore.EnrichmentStatus
import org.resonate.android.nativecore.EnrichmentStop
import org.resonate.android.ui.counted

fun enrichmentLine(status: EnrichmentStatus?): String {
    if (status == null) return "Not looked up yet"
    if (status.running) {
        return if (status.due > 0u)
            "Finding artwork and details · ${status.asked} of ${status.due}"
        else "Finding artwork and details"
    }
    return when (status.stopped) {
        EnrichmentStop.FINISHED -> "Artwork and details up to date"
        EnrichmentStop.CANCELLED -> "Stopped"
        EnrichmentStop.OFFLINE -> "Stopped: offline"
        EnrichmentStop.UNREACHABLE -> "Stopped: MusicBrainz could not be reached"
        EnrichmentStop.REFUSED -> "Stopped: the service refused too many requests"
        EnrichmentStop.FAILED -> "Stopped: the library could not be read"
        null -> "Not looked up yet"
    }
}

fun enrichmentFound(status: EnrichmentStatus?): String? {
    if (status == null) return null
    val parts =
        listOfNotNull(
            status.matched.takeIf { it > 0u }?.let { "${counted(it, "song")} matched" },
            status.covers.takeIf { it > 0u }?.let { counted(it, "cover") },
            status.portraits.takeIf { it > 0u }?.let { counted(it, "portrait") },
            status.lyrics.takeIf { it > 0u }?.let { "lyrics for ${counted(it, "song")}" },
        )
    return parts.takeIf { it.isNotEmpty() }?.joinToString(" · ")?.replaceFirstChar { it.uppercase() }
}

fun enrichmentProgress(status: EnrichmentStatus?): Float? {
    if (status == null || !status.running || status.due == 0u) return null
    return (status.asked.toFloat() / status.due.toFloat()).coerceIn(0f, 1f)
}

fun coversLine(asked: UInt?): String =
    if (asked == null) "Ask again for albums still without a cover"
    else if (asked == 0u) "No album is waiting for a cover"
    else "Asking again for ${counted(asked, "album")}"
