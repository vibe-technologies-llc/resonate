package org.resonate.android

data class SpotifyAccount(val clientId: String = "", val refreshToken: String = "") {
    val signedIn: Boolean
        get() = clientId.isNotBlank() && refreshToken.isNotBlank()
}

object SpotifyWords {
    const val WHAT_FOLLOWING_DOES =
        "Follow keeps the liked songs and every playlist in step, read again every six hours. Move copies them once and follows nothing. A song you lack is saved for later, found by its ISRC or a strict search; one neither names is left out."
    const val WHAT_SPOTIFY_ALLOWS =
        "Spotify lets an app in development mode read only the playlists the account owns or collaborates on, so a playlist it merely follows is read from its public page: the first hundred songs at most, and not at all where it is private."
    const val SIGNED_IN = "Signed in to Spotify"
    const val SIGNED_OUT = "Signed out of Spotify: its playlists keep their songs and no longer follow it"

    fun canSignIn(clientId: String): Boolean = clientId.isNotBlank()

    fun register(redirect: String): String =
        "Register an app at developer.spotify.com with $redirect among its redirect URIs"
}
