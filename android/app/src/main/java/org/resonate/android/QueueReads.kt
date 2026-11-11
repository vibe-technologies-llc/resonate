package org.resonate.android

private const val CATALOG_REREAD_AFTER_MS = 3000L

data class QueueAsk(
    val zone: UInt,
    val queueRevision: ULong,
    val catalogRevision: ULong,
    val playing: UInt,
)

data class QueueShown(val heard: Int = HEARD_AT_MOST, val ahead: Int = AHEAD_AT_MOST) {
    fun widened(earlier: Boolean): QueueShown =
        if (earlier) copy(heard = heard + WIDENED_BY) else copy(ahead = ahead + WIDENED_BY)

    companion object {
        const val HEARD_AT_MOST = 50
        const val AHEAD_AT_MOST = 200
        const val WIDENED_BY = 200
    }
}

class QueueReads(private val catalogRereadAfterMs: Long = CATALOG_REREAD_AFTER_MS) {
    private var last: Reading? = null

    fun due(asked: QueueAsk, shown: QueueShown, nowMs: Long): Boolean {
        val read = last ?: return true
        return read.asked.zone != asked.zone ||
            read.asked.queueRevision != asked.queueRevision ||
            read.asked.playing != asked.playing ||
            read.shown != shown ||
            (read.asked.catalogRevision != asked.catalogRevision &&
                nowMs - read.atMs >= catalogRereadAfterMs)
    }

    fun noted(asked: QueueAsk, shown: QueueShown, nowMs: Long) {
        last = Reading(asked, shown, nowMs)
    }

    private class Reading(val asked: QueueAsk, val shown: QueueShown, val atMs: Long)
}
