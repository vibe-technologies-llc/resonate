package org.resonate.android

data class SpotifyAccount(val clientId: String = "", val refreshToken: String = "") {
    val signedIn: Boolean
        get() = clientId.isNotBlank() && refreshToken.isNotBlank()
}

object SpotifyWords {
    fun canSignIn(clientId: String): Boolean = clientId.isNotBlank()

    fun register(words: Words, redirect: String): String =
        words.string(R.string.spotify_words_register, redirect)
}
