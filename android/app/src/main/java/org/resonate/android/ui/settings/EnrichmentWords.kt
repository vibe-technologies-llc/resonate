package org.resonate.android.ui.settings

import org.resonate.android.R
import org.resonate.android.Words
import org.resonate.android.nativecore.EnrichmentStatus
import org.resonate.android.nativecore.EnrichmentStop

fun enrichmentLine(words: Words, status: EnrichmentStatus?): String {
    if (status == null) return words.string(R.string.enrichment_not_yet)
    if (status.running) {
        return if (status.due > 0u)
            words.string(
                R.string.enrichment_finding_counted,
                status.asked.toLong(),
                status.due.toLong(),
            )
        else words.string(R.string.enrichment_finding)
    }
    return when (status.stopped) {
        EnrichmentStop.FINISHED -> words.string(R.string.enrichment_finished)
        EnrichmentStop.CANCELLED -> words.string(R.string.enrichment_cancelled)
        EnrichmentStop.OFFLINE -> words.string(R.string.enrichment_offline)
        EnrichmentStop.UNREACHABLE -> words.string(R.string.enrichment_unreachable)
        EnrichmentStop.REFUSED -> words.string(R.string.enrichment_refused)
        EnrichmentStop.FAILED -> words.string(R.string.enrichment_failed)
        null -> words.string(R.string.enrichment_not_yet)
    }
}

fun enrichmentFound(words: Words, status: EnrichmentStatus?): String? {
    if (status == null) return null
    fun counted(id: Int, count: UInt) = words.plural(id, count.toInt(), count.toInt())
    val parts =
        listOfNotNull(
            status.matched.takeIf { it > 0u }?.let { counted(R.plurals.enrichment_matched, it) },
            status.covers.takeIf { it > 0u }?.let { counted(R.plurals.enrichment_covers, it) },
            status.portraits
                .takeIf { it > 0u }
                ?.let { counted(R.plurals.enrichment_portraits, it) },
            status.lyrics.takeIf { it > 0u }?.let { counted(R.plurals.enrichment_lyrics, it) },
        )
    return parts
        .takeIf { it.isNotEmpty() }
        ?.joinToString(" · ")
        ?.replaceFirstChar { it.uppercase() }
}

fun enrichmentProgress(status: EnrichmentStatus?): Float? {
    if (status == null || !status.running || status.due == 0u) return null
    return (status.asked.toFloat() / status.due.toFloat()).coerceIn(0f, 1f)
}

fun coversLine(words: Words, asked: UInt?): String =
    if (asked == null) words.string(R.string.enrichment_covers_ask)
    else if (asked == 0u) words.string(R.string.enrichment_covers_none_waiting)
    else words.plural(R.plurals.enrichment_covers_asking, asked.toInt(), asked.toInt())
