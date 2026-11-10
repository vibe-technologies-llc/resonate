package org.resonate.android.ui.library

import androidx.annotation.StringRes
import org.resonate.android.R
import org.resonate.android.Words
import org.resonate.android.nativecore.TrackFilter
import org.resonate.android.nativecore.TrackOrder
import org.resonate.android.ui.kilohertz

enum class FilterPart {
    GENRE,
    ARTIST,
    ALBUM,
    RATE,
    DEPTH,
    FAVOURITES,
    UNPLAYED,
}

@get:StringRes
val TrackOrder.label: Int
    get() =
        when (this) {
            TrackOrder.TITLE -> R.string.library_order_title
            TrackOrder.ALBUM -> R.string.library_order_album
            TrackOrder.ARTIST -> R.string.library_order_artist
            TrackOrder.DURATION -> R.string.library_order_length
            TrackOrder.ADDED -> R.string.library_order_added
            TrackOrder.PLAYS -> R.string.library_order_most_played
            TrackOrder.PLAYED -> R.string.library_order_last_played
        }

val sampleRates = listOf(0u, 44_100u, 88_200u, 96_000u, 192_000u)

val bitDepths = listOf(0u, 16u, 24u, 32u)

fun rateLabel(words: Words, rate: UInt): String =
    if (rate == 0u) words.string(R.string.library_filter_any) else kilohertz(words, rate)

fun depthLabel(words: Words, bits: UInt): String =
    if (bits == 0u) words.string(R.string.library_filter_any)
    else words.string(R.string.unit_bits, bits.toInt())

fun TrackFilter.parts(): List<FilterPart> =
    FilterPart.entries.filter { part ->
        when (part) {
            FilterPart.GENRE -> genre.isNotBlank()
            FilterPart.ARTIST -> artist.isNotBlank()
            FilterPart.ALBUM -> album.isNotBlank()
            FilterPart.RATE -> minimumRate > 0u
            FilterPart.DEPTH -> minimumBits > 0u
            FilterPart.FAVOURITES -> favourites
            FilterPart.UNPLAYED -> unplayed
        }
    }

fun TrackFilter.label(words: Words, part: FilterPart): String =
    when (part) {
        FilterPart.GENRE -> genre
        FilterPart.ARTIST -> artist
        FilterPart.ALBUM -> album
        FilterPart.RATE -> words.string(R.string.library_filter_rate_at_least, kilohertz(words, minimumRate))
        FilterPart.DEPTH -> words.string(R.string.library_filter_bits_at_least, minimumBits.toInt())
        FilterPart.FAVOURITES -> words.string(R.string.library_filter_favourites)
        FilterPart.UNPLAYED -> words.string(R.string.library_filter_unplayed)
    }

fun TrackFilter.without(part: FilterPart): TrackFilter =
    when (part) {
        FilterPart.GENRE -> copy(genre = "")
        FilterPart.ARTIST -> copy(artist = "")
        FilterPart.ALBUM -> copy(album = "")
        FilterPart.RATE -> copy(minimumRate = 0u)
        FilterPart.DEPTH -> copy(minimumBits = 0u)
        FilterPart.FAVOURITES -> copy(favourites = false)
        FilterPart.UNPLAYED -> copy(unplayed = false)
    }

fun TrackFilter.trimmed(): TrackFilter =
    copy(genre = genre.trim(), artist = artist.trim(), album = album.trim())

val TrackFilter.ordered: Boolean
    get() = order != TrackOrder.TITLE || descending

fun TrackFilter.orderLabel(words: Words): String =
    if (descending) words.string(R.string.library_order_descending, words.string(order.label))
    else words.string(order.label)

fun suggestions(values: Sequence<String>, typed: String, atMost: Int = 6): List<String> {
    val wanted = typed.trim()
    return values
        .filter { it.isNotBlank() }
        .distinctBy { it.lowercase() }
        .filter { wanted.isEmpty() || it.contains(wanted, ignoreCase = true) }
        .filter { !it.equals(wanted, ignoreCase = true) }
        .sortedWith(compareBy<String> { !it.startsWith(wanted, ignoreCase = true) }.thenBy { it.lowercase() })
        .take(atMost)
        .toList()
}
