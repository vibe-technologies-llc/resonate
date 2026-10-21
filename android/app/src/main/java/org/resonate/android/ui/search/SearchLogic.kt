package org.resonate.android.ui.search

import java.util.Locale
import org.resonate.android.nativecore.LibrarySearch
import org.resonate.android.nativecore.SearchColumn
import org.resonate.android.nativecore.TextRange
import org.resonate.android.ui.shownTitle

const val RECENT_SEARCHES_KEPT = 10
const val LIBRARY_SONGS_AT_MOST = 500u
const val ONLINE_TEXT_AT_LEAST = 2
const val COMPLETIONS_SHOWN = 5
const val TOP_SONGS = 5
const val TOP_SHELF = 8

private val linkPrefixes = listOf("http://", "https://", "spotify:")

fun isLink(text: String): Boolean {
    val lowered = text.trim().lowercase(Locale.ROOT)
    return linkPrefixes.any { lowered.startsWith(it) }
}

fun keptSearch(recent: List<String>, text: String): List<String> {
    val query = text.trim()
    if (query.isEmpty() || isLink(query)) return recent
    val others = recent.filterNot { it.equals(query, ignoreCase = true) }
    return (listOf(query) + others).take(RECENT_SEARCHES_KEPT)
}

fun forgottenSearch(recent: List<String>, text: String): List<String> = recent.filterNot { it == text }

fun offeredCompletions(
    completions: List<String>,
    answered: String,
    fold: (String) -> String,
): List<String> {
    val typed = fold(answered)
    return completions
        .filter { completion ->
            val folded = fold(completion)
            folded != typed && folded.startsWith(typed)
        }
        .distinct()
        .take(COMPLETIONS_SHOWN)
}

fun litRanges(text: String, ranges: List<TextRange>): List<IntRange> {
    val units = text.length
    return ranges
        .mapNotNull { range ->
            var start = range.start.toLong().coerceAtMost(units.toLong()).toInt()
            var end = range.end.toLong().coerceAtMost(units.toLong()).toInt()
            if (start >= end) return@mapNotNull null
            if (start > 0 && text[start].isLowSurrogate() && text[start - 1].isHighSurrogate()) {
                start -= 1
            }
            if (end < units && text[end].isLowSurrogate() && text[end - 1].isHighSurrogate()) {
                end += 1
            }
            start until end
        }
        .sortedBy { it.first }
        .fold(emptyList()) { merged, range ->
            val last = merged.lastOrNull()
            if (last != null && range.first <= last.last + 1)
                merged.dropLast(1) + listOf(last.first..maxOf(last.last, range.last))
            else merged + listOf(range)
        }
}

data class Lit(private val ranges: Map<Pair<SearchColumn, String>, List<IntRange>>) {
    fun of(column: SearchColumn, text: String): List<IntRange> = ranges[column to text].orEmpty()

    companion object {
        val none = Lit(emptyMap())
    }
}

fun asked(found: LibrarySearch): Set<Pair<SearchColumn, String>> =
    buildSet {
            found.tracks.forEach { track ->
                add(SearchColumn.TITLE to track.shownTitle)
                add(SearchColumn.ARTIST to track.artist)
                add(SearchColumn.ALBUM to track.album)
            }
            found.albums.forEach { album ->
                add(SearchColumn.ALBUM to album.title)
                add(SearchColumn.ARTIST to album.artist)
            }
            found.artists.forEach { artist -> add(SearchColumn.ARTIST to artist.name) }
        }
        .filterTo(mutableSetOf()) { it.second.isNotBlank() }

fun lit(
    query: String,
    found: LibrarySearch,
    highlights: (String, String, SearchColumn) -> List<TextRange>,
): Lit {
    if (query.isBlank()) return Lit.none
    return Lit(
        asked(found)
            .associateWith { (column, text) -> litRanges(text, highlights(query, text, column)) }
            .filterValues { it.isNotEmpty() }
    )
}

enum class LibraryScope {
    TOP,
    SONGS,
    ALBUMS,
    ARTISTS,
}

fun shownScope(chosen: LibraryScope, found: LibrarySearch): LibraryScope =
    when (chosen) {
        LibraryScope.ALBUMS -> if (found.albums.isEmpty()) LibraryScope.TOP else chosen
        LibraryScope.ARTISTS -> if (found.artists.isEmpty()) LibraryScope.TOP else chosen
        LibraryScope.SONGS -> if (found.tracks.isEmpty()) LibraryScope.TOP else chosen
        LibraryScope.TOP -> chosen
    }

fun scopes(found: LibrarySearch): List<LibraryScope> = buildList {
    add(LibraryScope.TOP)
    if (found.tracks.isNotEmpty()) add(LibraryScope.SONGS)
    if (found.albums.isNotEmpty()) add(LibraryScope.ALBUMS)
    if (found.artists.isNotEmpty()) add(LibraryScope.ARTISTS)
}

fun scopeLabel(scope: LibraryScope, found: LibrarySearch): String =
    when (scope) {
        LibraryScope.TOP -> "Top results"
        LibraryScope.SONGS -> "Songs · ${songsCounted(found)}"
        LibraryScope.ALBUMS -> "Albums · ${found.albums.size}"
        LibraryScope.ARTISTS -> "Artists · ${found.artists.size}"
    }

fun songsCounted(found: LibrarySearch): String {
    val shown = found.tracks.size
    val total = found.trackCount.toLong()
    return if (total > shown) "$shown of $total" else "$total"
}

fun matchedNothing(found: LibrarySearch): Boolean =
    found.tracks.isEmpty() && found.albums.isEmpty() && found.artists.isEmpty()
