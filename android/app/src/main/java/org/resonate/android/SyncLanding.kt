package org.resonate.android

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
    const val AWAY =
        "The folder songs from your devices go to cannot be reached; they wait until it can"
    const val NOWHERE = "Add a music folder with write access to keep songs from your devices"

    fun destination(folders: List<LandingFolder>, chosen: String?): LandingDestination {
        val folder =
            folders.find { it.id == chosen && it.writable }
                ?: folders.firstOrNull { it.writable && !it.downloads }
                ?: return LandingDestination.Nowhere
        return if (folder.readable) LandingDestination.Into(folder.id) else LandingDestination.Away
    }
}
