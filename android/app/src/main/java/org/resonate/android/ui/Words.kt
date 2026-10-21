package org.resonate.android.ui

import java.util.Locale
import org.resonate.android.nativecore.Track

val Track.shownTitle: String
    get() = title.ifBlank { name.ifBlank { "Unknown song" } }

val Track.shownArtist: String
    get() = artist.ifBlank { "Unknown artist" }

val Track.albumOwner: String
    get() = albumArtist.ifBlank { artist }

val Track.byline: String
    get() = if (album.isBlank()) shownArtist else "$shownArtist · $album"

val Track.quality: String
    get() =
        listOfNotNull(
                codec.takeIf { it.isNotBlank() },
                rate.takeIf { it > 0u }?.let { kilohertz(it) },
                bits?.let { "$it bit" },
            )
            .joinToString(" · ")

fun kilohertz(rate: UInt): String {
    val khz = rate.toDouble() / 1000
    return if (khz % 1.0 == 0.0) "${khz.toInt()} kHz"
    else String.format(Locale.ROOT, "%.1f kHz", khz)
}

fun clock(milliseconds: Long): String {
    val seconds = (milliseconds / 1000).coerceAtLeast(0)
    val hours = seconds / 3600
    val minutes = (seconds % 3600) / 60
    val rest = seconds % 60
    return if (hours > 0) String.format(Locale.ROOT, "%d:%02d:%02d", hours, minutes, rest)
    else String.format(Locale.ROOT, "%d:%02d", minutes, rest)
}

fun clock(milliseconds: ULong): String = clock(milliseconds.toLong())

fun length(milliseconds: ULong): String {
    val minutes = (milliseconds.toLong() / 60_000).coerceAtLeast(0)
    return when {
        minutes < 1 -> "under a minute"
        minutes < 60 -> "$minutes min"
        else -> "${minutes / 60} hr ${minutes % 60} min"
    }
}

fun counted(count: Long, noun: String, plural: String = "${noun}s"): String =
    "$count ${if (count == 1L) noun else plural}"

fun counted(count: Int, noun: String, plural: String = "${noun}s"): String =
    counted(count.toLong(), noun, plural)

fun counted(count: UInt, noun: String, plural: String = "${noun}s"): String =
    counted(count.toLong(), noun, plural)

fun counted(count: ULong, noun: String, plural: String = "${noun}s"): String =
    counted(count.toLong(), noun, plural)

fun songsAndLength(tracks: List<Track>): String =
    "${counted(tracks.size, "song")} · ${length(tracks.sumOf { it.durationMs })}"

fun humanized(name: String): String =
    name.lowercase(Locale.ROOT).replace('_', ' ').replaceFirstChar { it.titlecase(Locale.ROOT) }

fun queueLanding(from: Int, to: Int): Int = if (to > from) to + 1 else to
