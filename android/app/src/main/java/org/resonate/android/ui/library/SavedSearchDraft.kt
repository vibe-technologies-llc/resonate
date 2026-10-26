package org.resonate.android.ui.library

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

val SearchOrder.label: String
    get() =
        when (this) {
            SearchOrder.RELEVANCE -> "Relevance"
            SearchOrder.ALBUM -> "Album"
            SearchOrder.TITLE -> "Title"
            SearchOrder.ARTIST -> "Artist"
            SearchOrder.ADDED -> "Date added"
            SearchOrder.DURATION -> "Duration"
            SearchOrder.PLAYS -> "Plays"
            SearchOrder.PLAYED -> "Last played"
            SearchOrder.FAVOURITES -> "Favourites"
            SearchOrder.PLAYS_THIS_MONTH -> "Plays this month"
        }

fun SavedSearch.reading(): String {
    val direction =
        when {
            descending == searchOrderReadsDescending(order) -> ""
            descending -> ", descending"
            else -> ", ascending"
        }
    val limit = rows?.let { " · first $it" }.orEmpty()
    return "${order.label}$direction$limit"
}
