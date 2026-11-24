package org.resonate.android

enum class RowPresence {
    Standing,
    Absent,
    Unknown,
}

object MediaStoreRemoval {
    fun presence(firstRow: () -> Boolean?): RowPresence =
        try {
            when (firstRow()) {
                true -> RowPresence.Standing
                false -> RowPresence.Absent
                null -> RowPresence.Unknown
            }
        } catch (denied: SecurityException) {
            RowPresence.Unknown
        }

    fun settled(removed: Int, presence: () -> RowPresence): Boolean =
        removed > 0 || presence() == RowPresence.Absent
}
