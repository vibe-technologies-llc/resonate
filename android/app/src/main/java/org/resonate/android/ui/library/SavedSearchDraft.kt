package org.resonate.android.ui.library

import androidx.annotation.StringRes
import org.resonate.android.R
import org.resonate.android.Words
import org.resonate.android.nativecore.SavedSearch
import org.resonate.android.nativecore.SearchOrder
import org.resonate.android.nativecore.searchOrderReadsDescending

data class Cap(val rows: UInt?)

data class SavedSearchDraft(
    val name: String,
    val text: String,
    val order: SearchOrder,
    val descending: Boolean,
    val cap: String,
) {
    fun search(): SavedSearch? =
        capped(cap)?.let { SavedSearch(text = text.trim(), order = order, descending = descending, rows = it.rows) }

    fun ordered(chosen: SearchOrder): SavedSearchDraft =
        copy(order = chosen, descending = searchOrderReadsDescending(chosen))

    companion object {
        fun starting(name: String, text: String): SavedSearchDraft =
            SavedSearchDraft(
                name = name,
                text = text,
                order = SearchOrder.RELEVANCE,
                descending = searchOrderReadsDescending(SearchOrder.RELEVANCE),
                cap = "",
            )

        fun of(name: String, search: SavedSearch): SavedSearchDraft =
            SavedSearchDraft(
                name = name,
                text = search.text,
                order = search.order,
                descending = search.descending,
                cap = search.rows?.toString().orEmpty(),
            )
    }
}

fun capped(typed: String): Cap? {
    val digits = typed.trim()
    if (digits.isEmpty()) return Cap(null)
    if (!digits.all { it in '0'..'9' }) return null
    val rows = digits.toUIntOrNull() ?: return null
    return if (rows == 0u) null else Cap(rows)
}

@get:StringRes
val SearchOrder.label: Int
    get() =
        when (this) {
            SearchOrder.RELEVANCE -> R.string.library_search_order_relevance
            SearchOrder.ALBUM -> R.string.library_order_album
            SearchOrder.TITLE -> R.string.library_order_title
            SearchOrder.ARTIST -> R.string.library_order_artist
            SearchOrder.ADDED -> R.string.library_order_added
            SearchOrder.DURATION -> R.string.library_search_order_duration
            SearchOrder.PLAYS -> R.string.library_search_order_plays
            SearchOrder.PLAYED -> R.string.library_order_last_played
            SearchOrder.FAVOURITES -> R.string.library_search_order_favourites
            SearchOrder.PLAYS_THIS_MONTH -> R.string.library_search_order_plays_this_month
            SearchOrder.FORMAT -> R.string.library_search_order_format
        }

fun SavedSearch.reading(words: Words): String {
    val named = words.string(order.label)
    val directed =
        when {
            descending == searchOrderReadsDescending(order) -> named
            descending -> words.string(R.string.library_order_descending, named)
            else -> words.string(R.string.library_order_ascending, named)
        }
    return rows?.let { words.string(R.string.library_search_first, directed, it.toInt()) } ?: directed
}
