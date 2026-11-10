package org.resonate.android

import androidx.annotation.StringRes

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
}
