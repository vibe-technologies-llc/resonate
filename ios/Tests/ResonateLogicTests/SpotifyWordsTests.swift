import Testing

@testable import ResonateLogic

@Suite struct SpotifyWordsTests {
    @Test func anAccountIsSignedInOnlyWithBothHalves() {
        #expect(!SpotifyAccount().signedIn)
        #expect(!SpotifyAccount(clientId: "registered").signedIn)
        #expect(!SpotifyAccount(refreshToken: "token").signedIn)
        #expect(!SpotifyAccount(clientId: " ", refreshToken: "token").signedIn)
        #expect(SpotifyAccount(clientId: "registered", refreshToken: "token").signedIn)
    }

    @Test func aSignInIsOfferedOnlyWithAClient() {
        #expect(!SpotifyWords.canSignIn(clientId: ""))
        #expect(!SpotifyWords.canSignIn(clientId: "  "))
        #expect(SpotifyWords.canSignIn(clientId: "registered"))
    }

    @Test func theRedirectToRegisterIsSaid() {
        #expect(SpotifyWords.register(redirect: "http://127.0.0.1:43879/spotify").contains("http://127.0.0.1:43879/spotify"))
    }
}
