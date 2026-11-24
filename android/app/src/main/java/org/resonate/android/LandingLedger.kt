package org.resonate.android

import androidx.annotation.StringRes
import org.resonate.android.nativecore.Exception as NativeFailure

data class RefusedLanding(val until: Long, val tries: Int)

class LandingFailed(@StringRes val said: Int) : Exception()

data class LandingWork(
    val waiting: Int,
    val unpublished: Int,
    val destination: LandingDestination,
    val failure: String?,
    val moving: Boolean,
) {
    val pending: Int
        get() = waiting + unpublished

    val reachable: Boolean
        get() = destination is LandingDestination.Into

    val progressing: Boolean
        get() = unpublished > 0 || (waiting > 0 && reachable)

    val transferring: Boolean
        get() = moving || progressing

    val heldBack: Boolean
        get() = waiting > 0 && !reachable

    fun notice(words: Words): String? =
        when {
            heldBack && destination == LandingDestination.Away -> words.string(SyncLanding.AWAY)
            heldBack -> words.string(SyncLanding.NOWHERE)
            else -> failure?.takeIf { pending > 0 }
        }

    fun pollEveryMs(foreground: Boolean, backgroundPasses: Int): Long =
        if (foreground || transferring || backgroundPasses > 0) QUICK_POLL_MS else QUIET_POLL_MS

    fun rereadsFolders(now: Long, readAt: Long): Boolean =
        heldBack && now - readAt >= FOLDERS_REREAD_EVERY_MS

    companion object {
        const val QUICK_POLL_MS = 2000L
        const val QUIET_POLL_MS = 30000L
        const val FOLDERS_REREAD_EVERY_MS = 30000L
    }
}

class LandingLedger {
    private val refused = mutableMapOf<ULong, RefusedLanding>()
    private val unpublished = linkedMapOf<String, Long>()
    private val publicationTries = mutableMapOf<String, Int>()

    var failure: String? = null
        private set

    val unpublishedCount: Int
        get() = unpublished.size

    fun settle(waiting: Collection<ULong>) {
        refused.keys.retainAll(waiting.toSet())
        if (waiting.isEmpty() && unpublished.isEmpty()) failure = null
    }

    fun isDue(id: ULong, now: Long): Boolean = (refused[id]?.until ?: 0L) <= now

    fun landed(id: ULong) {
        refused.remove(id)
    }

    fun refuse(id: ULong, now: Long, said: String): Boolean {
        failure = said
        val tries = (refused[id]?.tries ?: 0) + 1
        if (tries < LANDING_TRIES_AT_MOST) {
            refused[id] = RefusedLanding(now + LANDING_PAUSE_MS, tries)
            return false
        }
        refused.remove(id)
        return true
    }

    fun notPublished(document: String, now: Long, said: String): Boolean {
        failure = said
        val tries = (publicationTries[document] ?: 0) + 1
        if (tries >= PUBLICATION_TRIES_AT_MOST) {
            publicationTries.remove(document)
            unpublished.remove(document)
            return true
        }
        publicationTries[document] = tries
        unpublished[document] = now + PUBLICATION_PAUSE_MS
        return false
    }

    fun duePublications(now: Long): List<String> =
        unpublished.filterValues { it <= now }.keys.toList()

    fun published(document: String) {
        unpublished.remove(document)
        publicationTries.remove(document)
    }

    fun work(waiting: Int, destination: LandingDestination, moving: Boolean): LandingWork =
        LandingWork(waiting, unpublished.size, destination, failure, moving)

    companion object {
        const val LANDING_PAUSE_MS = 60000L
        const val LANDING_TRIES_AT_MOST = 3
        const val PUBLICATION_PAUSE_MS = 10000L
        const val PUBLICATION_TRIES_AT_MOST = 5

        fun said(words: Words, failure: Exception): String =
            when (failure) {
                is LandingFailed -> words.string(failure.said)
                is NativeFailure ->
                    failure.message ?: words.string(R.string.features_sync_landing_not_kept)
                else -> words.string(R.string.features_sync_landing_not_kept)
            }
    }
}
