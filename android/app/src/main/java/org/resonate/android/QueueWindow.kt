package org.resonate.android

internal object QueueWindow {
    fun windowIndex(row: Int, window: QueueRead): Int =
        (row - window.first).coerceIn(window.tracks.indices)

    fun row(windowIndex: Int, window: QueueRead): Int =
        window.first + windowIndex.coerceIn(window.tracks.indices)
}
