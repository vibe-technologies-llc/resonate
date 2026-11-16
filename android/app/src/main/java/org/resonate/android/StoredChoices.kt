package org.resonate.android

import android.content.SharedPreferences

object StoredChoices {
    const val MUSICBRAINZ_COPY = "sync-musicbrainz-copy-chosen"
    const val MUSICBRAINZ_COPY_AS_FIRST_SAVED = "sync-musicbrainz-copy"

    const val DOWNLOAD_ON_METERED = "download-on-metered"
    const val STALL_DOWNLOADS_ON_METERED = "stall-downloads-on-metered"

    fun musicbrainzCopy(stored: (String) -> Boolean?): Boolean = stored(MUSICBRAINZ_COPY) ?: true

    fun downloadOnMetered(stored: (String) -> Boolean?): Boolean =
        stored(DOWNLOAD_ON_METERED) ?: stored(STALL_DOWNLOADS_ON_METERED)?.not() ?: true
}

fun SharedPreferences.storedFlag(key: String): Boolean? = if (contains(key)) getBoolean(key, false) else null
