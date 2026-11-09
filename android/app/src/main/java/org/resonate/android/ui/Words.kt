package org.resonate.android.ui

import androidx.compose.runtime.Composable
import java.util.Locale
import org.resonate.android.R
import org.resonate.android.Words
import org.resonate.android.nativecore.Track
import org.resonate.android.words

fun Track.shownTitle(words: Words): String =
    title.ifBlank { name.ifBlank { words.string(R.string.unknown_song) } }

val Track.shownTitle: String
    @Composable get() = shownTitle(words())

fun Track.shownArtist(words: Words): String =
    artist.ifBlank { words.string(R.string.unknown_artist) }

val Track.shownArtist: String
    @Composable get() = shownArtist(words())

val Track.albumOwner: String
    get() = albumArtist.ifBlank { artist }

fun Track.byline(words: Words): String =
    if (album.isBlank()) shownArtist(words) else "${shownArtist(words)} · $album"

val Track.byline: String
    @Composable get() = byline(words())

fun Track.quality(words: Words): String =
    listOfNotNull(
            codec.takeIf { it.isNotBlank() },
            rate.takeIf { it > 0u }?.let { kilohertz(words, it) },
            bits?.let { words.string(R.string.unit_bits, it.toInt()) },
        )
        .joinToString(" · ")

val Track.quality: String
    @Composable get() = quality(words())

fun kilohertz(words: Words, rate: UInt): String {
    val khz = rate.toDouble() / 1000
    val figure =
        if (khz % 1.0 == 0.0) khz.toInt().toString() else String.format(Locale.ROOT, "%.1f", khz)
    return words.string(R.string.unit_kilohertz, figure)
}

@Composable fun kilohertz(rate: UInt): String = kilohertz(words(), rate)

fun clock(milliseconds: Long): String {
    val seconds = (milliseconds / 1000).coerceAtLeast(0)
    val hours = seconds / 3600
    val minutes = (seconds % 3600) / 60
    val rest = seconds % 60
    return if (hours > 0) String.format(Locale.ROOT, "%d:%02d:%02d", hours, minutes, rest)
    else String.format(Locale.ROOT, "%d:%02d", minutes, rest)
}

fun clock(milliseconds: ULong): String = clock(milliseconds.toLong())

fun length(words: Words, milliseconds: ULong): String {
    val minutes = (milliseconds.toLong() / 60_000).coerceAtLeast(0)
    return when {
        minutes < 1 -> words.string(R.string.length_under_a_minute)
        minutes < 60 -> words.string(R.string.length_minutes, minutes)
        else -> words.string(R.string.length_hours_and_minutes, minutes / 60, minutes % 60)
    }
}

@Composable fun length(milliseconds: ULong): String = length(words(), milliseconds)

fun songsAndLength(words: Words, tracks: List<Track>): String =
    words.plural(R.plurals.count_songs, tracks.size, tracks.size) +
        " · " +
        length(words, tracks.sumOf { it.durationMs })

@Composable fun songsAndLength(tracks: List<Track>): String = songsAndLength(words(), tracks)

fun humanized(name: String): String =
    name.lowercase(Locale.ROOT).replace('_', ' ').replaceFirstChar { it.titlecase(Locale.ROOT) }

fun queueLanding(from: Int, to: Int): Int = if (to > from) to + 1 else to
