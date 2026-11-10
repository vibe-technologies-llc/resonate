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
    val cancelling: Boolean = false,
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

    fun claimed(jobs: List<DownloadJob>, atOnce: Int = AT_ONCE): Pair<List<DownloadJob>, List<Long>> {
        val starting = startable(jobs, atOnce).toSet()
        val next =
            jobs.map { if (it.key in starting) it.copy(state = DownloadState.FINDING) else it }
        return next to starting.toList()
    }

    fun cancelled(jobs: List<DownloadJob>, key: Long): List<DownloadJob> =
        jobs.map {
            when {
                it.key != key || it.state.settled -> it
                it.state == DownloadState.WAITING -> it.copy(state = DownloadState.CANCELLED)
                else -> it.copy(cancelling = true)
            }
        }

    fun held(jobs: List<DownloadJob>): Set<String> =
        jobs.filter { !it.state.settled }.map { it.song.heldKey() }.toSet()

    fun standing(jobs: List<DownloadJob>, song: SearchSong): DownloadState? =
        jobs.firstOrNull { !it.state.settled && it.song.heldKey() == song.heldKey() }?.state

    fun stranded(journalled: Collection<String>, indexed: Set<String>): Set<String> =
        journalled.filter { it !in indexed }.toSet()

    fun received(jobs: List<DownloadJob>): ULong =
        jobs.filter { it.state.inFlight }.fold(0uL) { sum, job -> sum + job.received }

    fun summary(words: Words, jobs: List<DownloadJob>): String? {
        if (jobs.isEmpty()) return null
        val active = jobs.filter { !it.state.settled }
        if (active.isNotEmpty()) return working(words, jobs, active)
        return finished(words, jobs)
    }

    fun nextBatch(jobs: List<DownloadJob>): List<DownloadJob> =
        if (jobs.all { it.state.settled }) emptyList() else jobs

    fun meterLine(words: Words, waiting: Boolean): String? =
        if (waiting) words.string(R.string.transfer_waiting_unmetered) else null

    fun waitForWants(dueAtSeconds: Long, nowSeconds: Long): Long =
        ((dueAtSeconds - nowSeconds) * 1000L).coerceIn(WANTS_ASKED_AT_LEAST_MS, WANTS_ASKED_EVERY_MS)

    private fun working(words: Words, jobs: List<DownloadJob>, active: List<DownloadJob>): String {
        val current = active.firstOrNull { it.state.inFlight } ?: active.first()
        val title = current.song.title
        val doing =
            words.string(
                when (current.state) {
                    DownloadState.FINDING -> R.string.transfer_finding
                    DownloadState.VALIDATING -> R.string.transfer_checking
                    DownloadState.DOWNLOADING -> R.string.transfer_downloading
                    else -> R.string.transfer_waiting_for
                },
                title,
            )
        if (jobs.size == 1) return doing
        val done = jobs.size - active.size
        return words.string(R.string.transfer_batch_running, done + 1, jobs.size, doing)
    }

    private fun finished(words: Words, jobs: List<DownloadJob>): String {
        val done = jobs.filter { it.state == DownloadState.DONE }
        val failed = jobs.filter { it.state == DownloadState.FAILED }
        if (jobs.size == 1) {
            val only = jobs.single()
            return when (only.state) {
                DownloadState.DONE ->
                    words.string(
                        R.string.transfer_done_one,
                        only.name ?: only.song.title,
                        (only.received / 1024uL).toLong(),
                    )
                DownloadState.CANCELLED -> words.string(R.string.transfer_cancelled)
                else -> only.failure ?: words.string(R.string.transfer_failed)
            }
        }
        val downloaded =
            words.plural(R.plurals.transfer_done_batch, jobs.size, done.size, jobs.size)
        return if (failed.isEmpty()) downloaded
        else
            words.plural(
                R.plurals.transfer_done_batch_failed,
                failed.size,
                downloaded,
                failed.size,
                failed.first().failure ?: words.string(R.string.transfer_failed),
            )
    }
}
