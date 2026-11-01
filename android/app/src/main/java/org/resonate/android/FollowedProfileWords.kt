package org.resonate.android

import org.resonate.android.nativecore.FollowedProfile
import org.resonate.android.nativecore.MusicPlaylist
import org.resonate.android.ui.counted

data class ProfileChanges(val added: Int, val changed: Int, val gone: UInt) {
    val quiet: Boolean
        get() = added == 0 && changed == 0 && gone == 0u
}

object FollowedProfileWords {
    private fun nameOf(followed: FollowedProfile): String = followed.name.ifBlank { "this profile" }

    private fun leftOut(followed: FollowedProfile): List<String> =
        listOfNotNull(
            followed.letGo.takeIf { it > 0u }?.let { "$it let go" },
            followed.gone.takeIf { it > 0u }?.let { "$it deleted on ${followed.service}" },
            followed.unread.takeIf { it > 0u }?.let { "$it unreadable" },
        )

    fun followed(followed: FollowedProfile): String =
        (listOf(
                "Following ${nameOf(followed)} on ${followed.service} · ${counted(followed.playlists.size, "playlist")}"
            ) + leftOut(followed))
            .joinToString(", ")

    fun moved(followed: FollowedProfile): String =
        (listOf("Moved ${counted(followed.playlists.size, "playlist")} of ${nameOf(followed)} to Resonate") +
                listOfNotNull(followed.unread.takeIf { it > 0u }?.let { "$it unreadable" }))
            .joinToString(", ")

    fun changes(before: List<MusicPlaylist>, after: List<MusicPlaylist>, followed: FollowedProfile): ProfileChanges {
        val revisions = before.associate { it.id to it.revision }
        val touched = followed.playlists.map { it.id }.toSet()
        val now = after.filter { it.id in touched }
        return ProfileChanges(
            added = now.count { it.id !in revisions },
            changed = now.count { it.id in revisions && revisions[it.id] != it.revision },
            gone = followed.gone,
        )
    }

    fun refreshed(followed: FollowedProfile, changes: ProfileChanges): String {
        val parts =
            listOfNotNull(
                changes.added.takeIf { it > 0 }?.let { counted(it, "new playlist") },
                changes.changed.takeIf { it > 0 }?.let { "$it changed" },
                changes.gone.takeIf { it > 0u }?.let { "$it deleted on ${followed.service}" },
            )
        return "${nameOf(followed)} on ${followed.service}: ${parts.joinToString(" · ")}"
    }

    fun line(service: String?): String =
        "${service ?: "Followed"} profile · its playlists are kept in step"

    const val STOPPED = "No longer following that profile; its playlists stay"
}
