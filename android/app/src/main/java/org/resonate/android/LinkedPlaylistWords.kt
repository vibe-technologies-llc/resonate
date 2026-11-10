package org.resonate.android

import org.resonate.android.nativecore.PlaylistLinkInfo
import org.resonate.android.nativecore.RefreshedPlaylist

object LinkedPlaylistWords {
    fun imported(
        words: Words,
        name: String,
        service: String?,
        keptInStep: Boolean,
        matched: UInt,
        missing: UInt,
    ): String {
        if (!keptInStep || service == null) {
            return words.plural(
                R.plurals.linked_imported_local,
                matched.toInt(),
                matched.toInt(),
                missing.toInt(),
            )
        }
        val songs = words.plural(R.plurals.count_songs, matched.toInt(), matched.toInt())
        return if (missing == 0u) words.string(R.string.linked_keeping, name, service, songs)
        else words.plural(
            R.plurals.linked_keeping_and_saved,
            missing.toInt(),
            name,
            service,
            songs,
            missing.toInt(),
        )
    }

    fun naming(words: Words, keptInStep: Boolean): String =
        words.string(
            if (keptInStep) R.string.linked_naming_kept_in_step else R.string.linked_naming_import
        )

    fun line(words: Words, link: PlaylistLinkInfo): String =
        if (link.keptInStep) words.string(R.string.linked_line_kept_in_step, link.service)
        else words.string(R.string.linked_line_imported, link.service)

    fun moved(refreshed: RefreshedPlaylist): Boolean =
        refreshed.added > 0u || refreshed.removed > 0u || refreshed.wanted > 0u

    fun refreshed(
        words: Words,
        name: String,
        service: String,
        refreshed: RefreshedPlaylist,
    ): String {
        val parts =
            listOfNotNull(
                refreshed.added
                    .takeIf { it > 0u }
                    ?.let { words.plural(R.plurals.linked_songs_added, it.toInt(), it.toInt()) },
                refreshed.removed
                    .takeIf { it > 0u }
                    ?.let { words.plural(R.plurals.linked_taken_out, it.toInt(), it.toInt()) },
                refreshed.wanted
                    .takeIf { it > 0u }
                    ?.let { words.plural(R.plurals.linked_saved_for_later, it.toInt(), it.toInt()) },
            )
        return if (parts.isEmpty()) words.string(R.string.linked_nothing_new, name, service)
        else words.string(R.string.linked_refreshed, name, parts.joinToString(" · "))
    }

    fun inStep(words: Words, name: String, service: String, keptInStep: Boolean): String =
        if (keptInStep) words.string(R.string.linked_now_kept_in_step, name, service)
        else words.string(R.string.linked_no_longer_mirrors, name, service)

    fun forgotten(words: Words, name: String): String =
        words.string(R.string.linked_link_forgotten, name)
}
