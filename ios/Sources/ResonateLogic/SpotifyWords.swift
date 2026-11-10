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
    public static var signedIn: String { said("spotify.signed-in") }
    public static var signedOut: String { said("spotify.signed-out") }
    public static var whatFollowingDoes: String { said("spotify.following-explained") }
    public static var whatSpotifyAllows: String { said("spotify.allows") }

    static func isGiven(_ text: String) -> Bool {
        !text.allSatisfy(\.isWhitespace)
    }

    public static func canSignIn(clientId: String) -> Bool {
        isGiven(clientId)
    }

    public static func register(redirect: String) -> String {
        said("spotify.register", redirect)
    }
}
