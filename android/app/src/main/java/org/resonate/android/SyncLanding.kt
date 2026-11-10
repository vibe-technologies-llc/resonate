package org.resonate.android

import androidx.annotation.StringRes
import org.resonate.android.nativecore.SyncStanding
import org.resonate.android.nativecore.SyncStatus

data class LandingFolder(
    val id: String,
    val readable: Boolean,
    val writable: Boolean,
    val downloads: Boolean,
)

sealed interface LandingDestination {
    data class Into(val id: String) : LandingDestination

    data object Away : LandingDestination

    data object Nowhere : LandingDestination
}

object SyncLanding {
    @StringRes val AWAY = R.string.sync_landing_away
    @StringRes val NOWHERE = R.string.sync_landing_nowhere

    fun destination(folders: List<LandingFolder>, chosen: String?): LandingDestination {
        val folder =
            folders.find { it.id == chosen && it.writable }
                ?: folders.firstOrNull { it.writable && !it.downloads }
                ?: return LandingDestination.Nowhere
        return if (folder.readable) LandingDestination.Into(folder.id) else LandingDestination.Away
    }

    fun opened(heldBack: LandingDestination?, now: LandingDestination): Boolean =
        heldBack != null && heldBack !is LandingDestination.Into && now is LandingDestination.Into

    fun followsAgain(status: SyncStatus?): Boolean =
        status != null &&
            status.standing != SyncStanding.OFF &&
            status.moving == 0u &&
            status.settled

    fun beginning(words: Words, waiting: Int, folder: String): String =
        if (waiting > 0) words.plural(R.plurals.sync_landing_begins, waiting, waiting, folder)
        else words.string(R.string.sync_landing_asks_again, folder)

    fun keeping(words: Words, count: Int): String =
        words.plural(R.plurals.sync_landing_keeping, count, count)
}
