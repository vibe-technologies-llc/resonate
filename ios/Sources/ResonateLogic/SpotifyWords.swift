public struct SpotifyAccount: Equatable, Sendable {
    public var clientId: String
    public var refreshToken: String

    public init(clientId: String = "", refreshToken: String = "") {
        self.clientId = clientId
        self.refreshToken = refreshToken
    }

    public var signedIn: Bool {
        SpotifyWords.isGiven(clientId) && SpotifyWords.isGiven(refreshToken)
    }
}

public enum SpotifyWords {
    public static let neverCalledBack = "resonate-spotify-sign-in"
    public static let signedIn = "Signed in to Spotify"
    public static let signedOut = "Signed out of Spotify: its playlists keep their songs and no longer follow it"
    public static let whatFollowingDoes =
        "Follow keeps the liked songs and every playlist in step, read again every six hours. Move copies them once and follows nothing. A song you lack is saved for later, found by its ISRC or a strict search; one neither names is left out."
    public static let whatSpotifyAllows =
        "Spotify lets an app in development mode read only the playlists the account owns or collaborates on, so a playlist it merely follows is read from its public page: the first hundred songs at most, and not at all where it is private."

    static func isGiven(_ text: String) -> Bool {
        !text.allSatisfy(\.isWhitespace)
    }

    public static func canSignIn(clientId: String) -> Bool {
        isGiven(clientId)
    }

    public static func register(redirect: String) -> String {
        "Register an app at developer.spotify.com with \(redirect) among its redirect URIs"
    }
}
