package org.resonate.android

import android.content.SharedPreferences

object StoredChoices {
    const val MUSICBRAINZ_COPY = "sync-musicbrainz-copy-chosen"
    const val MUSICBRAINZ_COPY_AS_FIRST_SAVED = "sync-musicbrainz-copy"

    fun musicbrainzCopy(stored: (String) -> Boolean?): Boolean = stored(MUSICBRAINZ_COPY) ?: true
}

fun SharedPreferences.storedFlag(key: String): Boolean? = if (contains(key)) getBoolean(key, false) else null
