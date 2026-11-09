package org.resonate.android

import org.resonate.android.nativecore.FollowedProfile
import org.resonate.android.nativecore.MusicPlaylist

data class ProfileChanges(val added: Int, val changed: Int, val gone: UInt) {
    val quiet: Boolean
        get() = added == 0 && changed == 0 && gone == 0u
}

object FollowedProfileWords {
    private fun nameOf(words: Words, followed: FollowedProfile): String =
        followed.name.ifBlank { words.string(R.string.followed_this_profile) }

    private fun leftOut(words: Words, followed: FollowedProfile): List<String> =
        listOfNotNull(
            followed.letGo
                .takeIf { it > 0u }
                ?.let { words.plural(R.plurals.followed_let_go, it.toInt(), it.toInt()) },
            followed.gone
                .takeIf { it > 0u }
                ?.let { words.plural(R.plurals.followed_deleted_on, it.toInt(), it.toInt(), followed.service) },
            followed.unread
                .takeIf { it > 0u }
                ?.let { words.plural(R.plurals.followed_unreadable, it.toInt(), it.toInt()) },
        )

    fun followed(words: Words, followed: FollowedProfile): String {
        val count = followed.playlists.size
        val playlists = words.plural(R.plurals.count_playlists, count, count)
        val following =
            words.string(
                R.string.followed_following,
                nameOf(words, followed),
                followed.service,
                playlists,
            )
        return (listOf(following) + leftOut(words, followed)).joinToString(", ")
    }

    fun moved(words: Words, followed: FollowedProfile): String {
        val count = followed.playlists.size
        val moved = words.plural(R.plurals.followed_moved, count, count, nameOf(words, followed))
        val unread =
            followed.unread
                .takeIf { it > 0u }
                ?.let { words.plural(R.plurals.followed_unreadable, it.toInt(), it.toInt()) }
        return (listOf(moved) + listOfNotNull(unread)).joinToString(", ")
    }

    fun changes(
        before: List<MusicPlaylist>,
        after: List<MusicPlaylist>,
        followed: FollowedProfile,
    ): ProfileChanges {
        val revisions = before.associate { it.id to it.revision }
        val touched = followed.playlists.map { it.id }.toSet()
        val now = after.filter { it.id in touched }
        return ProfileChanges(
            added = now.count { it.id !in revisions },
            changed = now.count { it.id in revisions && revisions[it.id] != it.revision },
            gone = followed.gone,
        )
    }

    fun refreshed(words: Words, followed: FollowedProfile, changes: ProfileChanges): String {
        val parts =
            listOfNotNull(
                changes.added
                    .takeIf { it > 0 }
                    ?.let { words.plural(R.plurals.followed_new_playlists, it, it) },
                changes.changed
                    .takeIf { it > 0 }
                    ?.let { words.plural(R.plurals.followed_changed, it, it) },
                changes.gone
                    .takeIf { it > 0u }
                    ?.let {
                        words.plural(R.plurals.followed_deleted_on, it.toInt(), it.toInt(), followed.service)
                    },
            )
        return words.string(
            R.string.followed_refreshed,
            nameOf(words, followed),
            followed.service,
            parts.joinToString(" · "),
        )
    }

    fun line(words: Words, service: String?): String =
        if (service == null) words.string(R.string.followed_line_unnamed)
        else words.string(R.string.followed_line, service)
}
