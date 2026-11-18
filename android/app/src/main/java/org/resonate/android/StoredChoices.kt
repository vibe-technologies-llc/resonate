package org.resonate.android

import android.content.SharedPreferences
import org.resonate.android.nativecore.KeptHistory

object StoredChoices {
    const val MUSICBRAINZ_COPY = "sync-musicbrainz-copy-chosen"
    const val MUSICBRAINZ_COPY_AS_FIRST_SAVED = "sync-musicbrainz-copy"

    const val SYNC_DELETIONS_FROM_OTHERS = "sync-deletions-from-others"

    const val DOWNLOAD_ON_METERED = "download-on-metered"
    const val STALL_DOWNLOADS_ON_METERED = "stall-downloads-on-metered"

    const val CHECK_FOR_UPDATES = "check-for-updates"
    const val UPDATE_DISMISSED = "update-dismissed"

    const val HISTORY_KEPT_DAYS = "history-kept-days"

    fun musicbrainzCopy(stored: (String) -> Boolean?): Boolean = stored(MUSICBRAINZ_COPY) ?: true

    fun deletionsFromOthers(stored: (String) -> Boolean?): Boolean =
        stored(SYNC_DELETIONS_FROM_OTHERS) ?: true

    fun checkForUpdates(stored: (String) -> Boolean?): Boolean = stored(CHECK_FOR_UPDATES) ?: true

    fun downloadOnMetered(stored: (String) -> Boolean?): Boolean =
        stored(DOWNLOAD_ON_METERED) ?: stored(STALL_DOWNLOADS_ON_METERED)?.not() ?: true

    fun historyKept(stored: (String) -> Int?): KeptHistory =
        stored(HISTORY_KEPT_DAYS)
            ?.takeIf { it in 1..UShort.MAX_VALUE.toInt() }
            ?.let { KeptHistory.Days(it.toUShort()) } ?: KeptHistory.Forever

    fun historyKeptDays(kept: KeptHistory): Int? =
        when (kept) {
            is KeptHistory.Days -> kept.days.toInt()
            KeptHistory.Forever -> null
        }

    fun forgetsListens(now: KeptHistory, asked: KeptHistory): Boolean {
        val asking = historyKeptDays(asked) ?: return false
        val keeping = historyKeptDays(now) ?: return true
        return asking < keeping
    }
}

fun SharedPreferences.storedFlag(key: String): Boolean? = if (contains(key)) getBoolean(key, false) else null

fun SharedPreferences.storedNumber(key: String): Int? = if (contains(key)) getInt(key, 0) else null
