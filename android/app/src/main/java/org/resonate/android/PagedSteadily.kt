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

inline fun <T, K> pagedPinned(
    pageRows: UInt,
    keyOf: (T) -> K,
    stillWanted: () -> Boolean,
    page: (offset: UInt, limit: UInt, pinned: ULong?) -> Pair<List<T>, ULong>,
): List<T> {
    var rows = ArrayList<T>()
    for (walk in 1..PAGE_WALKS_AT_MOST) {
        rows = ArrayList()
        var pinned: ULong? = null
        var lost = false
        do {
            val (next, revision) = page(rows.size.toUInt(), pageRows, pinned)
            if (pinned != null && revision != pinned) {
                lost = true
                if (walk < PAGE_WALKS_AT_MOST) break
            }
            pinned = revision
            rows.addAll(next)
        } while (next.size == pageRows.toInt() && stillWanted())
        if (!lost || !stillWanted()) break
    }
    return rows.distinctBy(keyOf)
}
