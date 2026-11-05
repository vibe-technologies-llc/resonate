package org.resonate.android.ui.library

import org.resonate.android.nativecore.SearchSong
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

sealed interface AlbumRow {
    data class Held(val index: Int, val track: Track) : AlbumRow

    data class NotHeld(val song: SearchSong) : AlbumRow
}

data class Disc(val number: UInt?, val rows: List<AlbumRow>) {
    val held: List<IndexedValue<Track>>
        get() = rows.filterIsInstance<AlbumRow.Held>().map { IndexedValue(it.index, it.track) }
}

private const val FIRST_DISC = 1u

fun discsOf(tracks: List<Track>, notHeld: List<SearchSong> = emptyList()): List<Disc> {
    val indexed = tracks.withIndex().toList()
    val numbers = (tracks.mapNotNull { it.discNumber } + notHeld.map { it.disc ?: FIRST_DISC }).distinct()
    if (numbers.size < 2) return listOf(Disc(null, laidIn(indexed, notHeld)))
    val heldOn = indexed.groupBy { it.value.discNumber }
    val lackingOn = notHeld.groupBy { it.disc ?: FIRST_DISC }
    return discOrder(heldOn.keys, lackingOn.keys).map { number ->
        Disc(number, laidIn(heldOn[number].orEmpty(), lackingOn[number].orEmpty()))
    }
}

private fun discOrder(held: Set<UInt?>, lacking: Set<UInt>): List<UInt?> {
    val order = held.toMutableList()
    lacking.filter { it !in held }.sorted().forEach { number ->
        val after = order.indexOfFirst { it != null && it > number }
        order.add(if (after < 0) order.size else after, number)
    }
    return order
}

private fun laidIn(held: List<IndexedValue<Track>>, lacking: List<SearchSong>): List<AlbumRow> {
    val rows = held.map { AlbumRow.Held(it.index, it.value) }.toMutableList<AlbumRow>()
    lacking.forEach { song ->
        val place = song.position ?: UInt.MAX_VALUE
        val before = rows.indexOfFirst { placeOf(it) > place }
        rows.add(if (before < 0) rows.size else before, AlbumRow.NotHeld(song))
    }
    return rows
}

private fun placeOf(row: AlbumRow): UInt =
    when (row) {
        is AlbumRow.Held -> row.track.trackNumber ?: 0u
        is AlbumRow.NotHeld -> row.song.position ?: 0u
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
