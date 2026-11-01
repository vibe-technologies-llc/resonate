package org.resonate.android

import org.resonate.android.nativecore.PlaylistLinkInfo
import org.resonate.android.nativecore.RefreshedPlaylist
import org.resonate.android.ui.counted

object LinkedPlaylistWords {
    fun imported(
        name: String,
        service: String?,
        keptInStep: Boolean,
        matched: UInt,
        missing: UInt,
    ): String {
        if (!keptInStep || service == null) {
            return "Imported $matched local songs; $missing still missing"
        }
        val here = "Keeping “$name” in step with $service · ${counted(matched, "song")} here"
        return if (missing == 0u) here else "$here, $missing saved for later"
    }

    fun naming(keptInStep: Boolean): String =
        if (keptInStep) "Keep in step as a playlist" else "Import as playlist"

    fun line(link: PlaylistLinkInfo): String =
        if (link.keptInStep) "Kept in step with ${link.service}" else "Imported from ${link.service}"

    fun moved(refreshed: RefreshedPlaylist): Boolean =
        refreshed.added > 0u || refreshed.removed > 0u || refreshed.wanted > 0u

    fun refreshed(name: String, service: String, refreshed: RefreshedPlaylist): String {
        val parts =
            listOfNotNull(
                refreshed.added.takeIf { it > 0u }?.let { "${counted(it, "song")} added" },
                refreshed.removed.takeIf { it > 0u }?.let { "$it taken out" },
                refreshed.wanted.takeIf { it > 0u }?.let { "$it saved for later" },
            )
        return if (parts.isEmpty()) "Nothing new for “$name” on $service"
        else "“$name”: ${parts.joinToString(" · ")}"
    }

    fun inStep(name: String, service: String, keptInStep: Boolean): String =
        if (keptInStep) "“$name” is kept in step with $service"
        else "“$name” no longer mirrors $service; a refresh only adds what it lacks"

    fun forgotten(name: String): String = "“$name” no longer remembers its link"
}
