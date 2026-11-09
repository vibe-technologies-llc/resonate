package org.resonate.android

const val PAGE_WALKS_AT_MOST = 3

inline fun <T, K> pagedSteadily(
    pageRows: UInt,
    revision: () -> ULong,
    keyOf: (T) -> K,
    stillWanted: () -> Boolean,
    page: (offset: UInt, limit: UInt) -> List<T>,
): List<T> {
    var rows = ArrayList<T>()
    for (walk in 1..PAGE_WALKS_AT_MOST) {
        val before = revision()
        rows = ArrayList()
        do {
            val next = page(rows.size.toUInt(), pageRows)
            rows.addAll(next)
        } while (next.size == pageRows.toInt() && stillWanted())
        if (!stillWanted() || revision() == before) break
    }
    return rows.distinctBy(keyOf)
}
