package org.resonate.android.ui.library

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

val TrackOrder.label: String
    get() =
        when (this) {
            TrackOrder.TITLE -> "Title"
            TrackOrder.ALBUM -> "Album"
            TrackOrder.ARTIST -> "Artist"
            TrackOrder.DURATION -> "Length"
            TrackOrder.ADDED -> "Date added"
            TrackOrder.PLAYS -> "Most played"
            TrackOrder.PLAYED -> "Last played"
        }

val sampleRates = listOf(0u, 44_100u, 88_200u, 96_000u, 192_000u)

val bitDepths = listOf(0u, 16u, 24u, 32u)

fun rateLabel(rate: UInt): String = if (rate == 0u) "Any" else kilohertz(rate)

fun depthLabel(bits: UInt): String = if (bits == 0u) "Any" else "$bits bit"

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

fun TrackFilter.label(part: FilterPart): String =
    when (part) {
        FilterPart.GENRE -> genre
        FilterPart.ARTIST -> artist
        FilterPart.ALBUM -> album
        FilterPart.RATE -> "${kilohertz(minimumRate)} or more"
        FilterPart.DEPTH -> "$minimumBits bit or more"
        FilterPart.FAVOURITES -> "Favourites"
        FilterPart.UNPLAYED -> "Never played"
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

fun TrackFilter.orderLabel(): String = if (descending) "${order.label}, descending" else order.label

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
