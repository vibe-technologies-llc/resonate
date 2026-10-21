package org.resonate.android.ui.library

import org.resonate.android.nativecore.Track
import org.resonate.android.ui.counted
import org.resonate.android.ui.length

data class PlaylistEntry(val key: String, val position: Int, val track: Track)

fun playlistEntries(rows: List<Track>): List<PlaylistEntry> {
    val seen = HashMap<ULong, Int>()
    return rows.mapIndexed { position, track ->
        val occurrence = seen.merge(track.id, 1, Int::plus) ?: 1
        PlaylistEntry("${track.id}#$occurrence", position, track)
    }
}

data class Disc(val number: UInt?, val rows: List<IndexedValue<Track>>)

fun discsOf(tracks: List<Track>): List<Disc> {
    val indexed = tracks.withIndex().toList()
    val numbers = tracks.mapNotNull { it.discNumber }.distinct()
    if (numbers.size < 2) return listOf(Disc(null, indexed))
    return indexed
        .groupBy { it.value.discNumber }
        .map { (number, rows) -> Disc(number, rows) }
}

fun trackNumber(track: Track, index: Int): String = (track.trackNumber?.toString() ?: "${index + 1}")

fun discTitle(number: UInt?): String = if (number == null) "Other songs" else "Disc $number"

fun collectionSummary(vararg leading: String?, tracks: List<Track>): String =
    (leading.filterNotNull().filter { it.isNotBlank() } +
            counted(tracks.size, "song") +
            listOfNotNull(tracks.takeIf { it.isNotEmpty() }?.let { songs -> length(songs.sumOf { it.durationMs }) }))
        .joinToString(" · ")

fun greeting(hour: Int): String =
    when (hour) {
        in 5..11 -> "Good morning"
        in 12..17 -> "Good afternoon"
        else -> "Good evening"
    }

fun listenedHours(milliseconds: ULong): String {
    val hours = milliseconds.toLong() / 3_600_000
    val minutes = (milliseconds.toLong() / 60_000) % 60
    return if (hours > 0) "$hours h" else "$minutes min"
}

fun artistLine(kind: String, country: String, area: String): String =
    listOf(kind, area.ifBlank { country }).filter { it.isNotBlank() }.joinToString(" · ")

fun matchesWords(text: String, query: String): Boolean =
    query.split(' ', '\t').filter { it.isNotBlank() }.all { text.contains(it, ignoreCase = true) }
