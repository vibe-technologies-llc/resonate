package org.resonate.android

import org.resonate.android.nativecore.SearchSong

enum class DownloadState {
    WAITING,
    FINDING,
    DOWNLOADING,
    VALIDATING,
    DONE,
    FAILED,
    CANCELLED;

    val settled: Boolean
        get() = this == DONE || this == FAILED || this == CANCELLED

    val inFlight: Boolean
        get() = this == FINDING || this == DOWNLOADING || this == VALIDATING
}

data class DownloadJob(
    val key: Long,
    val song: SearchSong,
    val folder: String,
    val state: DownloadState = DownloadState.WAITING,
    val native: ULong? = null,
    val received: ULong = 0uL,
    val name: String? = null,
    val failure: String? = null,
)

fun SearchSong.heldKey(): String = recording.ifBlank { "$title\u001f$artist" }

object DownloadPlan {
    const val AT_ONCE = 2
    const val WANTS_ASKED_AT_LEAST_MS = 30_000L
    const val WANTS_ASKED_EVERY_MS = 30 * 60 * 1000L

    fun startable(jobs: List<DownloadJob>, atOnce: Int = AT_ONCE): List<Long> {
        val free = atOnce - jobs.count { it.state.inFlight }
        return jobs.filter { it.state == DownloadState.WAITING }.take(free.coerceAtLeast(0)).map { it.key }
    }

    fun held(jobs: List<DownloadJob>): Set<String> =
        jobs.filter { !it.state.settled }.map { it.song.heldKey() }.toSet()

    fun stranded(journalled: Collection<String>, indexed: Set<String>): Set<String> =
        journalled.filter { it !in indexed }.toSet()

    fun received(jobs: List<DownloadJob>): ULong =
        jobs.filter { it.state.inFlight }.fold(0uL) { sum, job -> sum + job.received }

    fun summary(jobs: List<DownloadJob>): String? {
        if (jobs.isEmpty()) return null
        val active = jobs.filter { !it.state.settled }
        if (active.isNotEmpty()) return working(jobs, active)
        return finished(jobs)
    }

    fun nextBatch(jobs: List<DownloadJob>): List<DownloadJob> =
        if (jobs.all { it.state.settled }) emptyList() else jobs

    fun waitForWants(dueAtSeconds: Long, nowSeconds: Long): Long =
        ((dueAtSeconds - nowSeconds) * 1000L).coerceIn(WANTS_ASKED_AT_LEAST_MS, WANTS_ASKED_EVERY_MS)

    private fun working(jobs: List<DownloadJob>, active: List<DownloadJob>): String {
        val current = active.firstOrNull { it.state.inFlight } ?: active.first()
        val title = current.song.title
        val verb =
            when (current.state) {
                DownloadState.FINDING -> "Finding"
                DownloadState.VALIDATING -> "Checking"
                DownloadState.DOWNLOADING -> "Downloading"
                else -> "Waiting for"
            }
        if (jobs.size == 1) return "$verb $title…"
        val done = jobs.size - active.size
        return "Downloading ${done + 1} of ${jobs.size} · $verb $title…"
    }

    private fun finished(jobs: List<DownloadJob>): String {
        val done = jobs.filter { it.state == DownloadState.DONE }
        val failed = jobs.filter { it.state == DownloadState.FAILED }
        if (jobs.size == 1) {
            val only = jobs.single()
            return when (only.state) {
                DownloadState.DONE ->
                    "Downloaded ${only.name ?: only.song.title} · ${only.received / 1024uL} KiB"
                DownloadState.CANCELLED -> "Download cancelled"
                else -> only.failure ?: "Download failed"
            }
        }
        val downloaded = "Downloaded ${done.size} of ${jobs.size} songs"
        return if (failed.isEmpty()) downloaded
        else "$downloaded · ${failed.size} failed: ${failed.first().failure ?: "Download failed"}"
    }
}
