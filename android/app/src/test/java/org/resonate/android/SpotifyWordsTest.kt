package org.resonate.android

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class SpotifyWordsTest {
    @Test
    fun anAccountIsSignedInOnlyWithBothHalves() {
        assertFalse(SpotifyAccount().signedIn)
        assertFalse(SpotifyAccount(clientId = "registered").signedIn)
        assertFalse(SpotifyAccount(refreshToken = "token").signedIn)
        assertFalse(SpotifyAccount(clientId = " ", refreshToken = "token").signedIn)
        assertTrue(SpotifyAccount(clientId = "registered", refreshToken = "token").signedIn)
    }

    @Test
    fun aSignInIsOfferedOnlyWithAClient() {
        assertFalse(SpotifyWords.canSignIn(""))
        assertFalse(SpotifyWords.canSignIn("  "))
        assertTrue(SpotifyWords.canSignIn("registered"))
    }

    @Test
    fun theRedirectToRegisterIsSaid() {
        assertTrue(
            SpotifyWords.register("http://127.0.0.1:43879/spotify")
                .contains("http://127.0.0.1:43879/spotify")
        )
    }
}
