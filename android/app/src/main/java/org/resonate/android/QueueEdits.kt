package org.resonate.android

data class RowMove(val from: Int, val to: Int)

object QueueEdits {
    fun appendedThenMoved(size: Int, index: Int, count: Int): List<RowMove> =
        if (index >= size) emptyList() else backward(size, index.coerceAtLeast(0), count)

    fun rangeMoved(size: Int, from: Int, until: Int, newIndex: Int): List<RowMove> {
        val first = from.coerceIn(0, size)
        val last = until.coerceIn(first, size)
        val count = last - first
        val target = newIndex.coerceIn(0, size - count)
        return when {
            count == 0 || target == first -> emptyList()
            target < first -> backward(first, target, count)
            else -> backward(last, first, target - first)
        }
    }

    fun rowsIn(queueIds: List<ULong?>, from: Int, until: Int): List<ULong> {
        val first = from.coerceIn(0, queueIds.size)
        return queueIds.subList(first, until.coerceIn(first, queueIds.size)).filterNotNull()
    }

    private fun backward(source: Int, destination: Int, count: Int): List<RowMove> =
        List(count) { RowMove(source + it, destination + it) }
}
