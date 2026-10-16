package org.resonate.android

private const val CATALOG_REREAD_AFTER_MS = 3000L

class QueueReads(private val catalogRereadAfterMs: Long = CATALOG_REREAD_AFTER_MS) {
    private var last: Reading? = null

    fun due(queueRevision: ULong, catalogRevision: ULong, nowMs: Long): Boolean {
        val read = last ?: return true
        return read.queueRevision != queueRevision ||
            (read.catalogRevision != catalogRevision && nowMs - read.atMs >= catalogRereadAfterMs)
    }

    fun noted(queueRevision: ULong, catalogRevision: ULong, nowMs: Long) {
        last = Reading(queueRevision, catalogRevision, nowMs)
    }

    private class Reading(val queueRevision: ULong, val catalogRevision: ULong, val atMs: Long)
}
