import Foundation
import ResonateNative

public enum Root: String, Hashable, Codable, Sendable {
    case home, library, settings, search
}

public enum Destination: Hashable, Codable, Sendable {
    case album(String)
    case artist(String)
    case playlist(UInt64)
    case playlists
    case albums
    case artists
    case favourites
    case tools
    case folders
    case suggestions
    case mix(String)
    case statistics
    case wanted
    case providers
    case scrobbling
    case spotify
    case sync
    case server
    case output
    case sound
    case processing
    case songLyrics(UInt64?)
    case visualizer
    case songAnalysis(UInt64?)
    case songDetails(UInt64)
    case appearance
    case background
    case identicalSongs
    case songs
    case served(ServedPlace)
    case party
    case headphoneCorrection
    case microphoneMeasurement
    case roomCorrection
    case listeningTest
    case otherOutputs
    case enrichment
    case searchHistory
    case notices
    case syncSongs
    case syncConnection
    case syncControl
    case syncDevice(String)

    public static func lyrics(_ track: Track?) -> Destination {
        .songLyrics(track?.id)
    }

    public static func analysis(_ track: Track?) -> Destination {
        .songAnalysis(track?.id)
    }

    public static func inspector(_ track: Track) -> Destination {
        .songDetails(track.id)
    }

    public var drawsItsOwnBackdrop: Bool {
        switch self {
        case .album, .artist, .playlist, .mix, .songLyrics, .visualizer: true
        default: false
        }
    }

    public var belongsToSettings: Bool {
        switch self {
        case .sound, .processing, .output, .scrobbling, .spotify, .providers, .sync, .server, .folders, .appearance,
            .background, .headphoneCorrection, .microphoneMeasurement, .roomCorrection, .listeningTest, .otherOutputs,
            .enrichment, .searchHistory, .notices, .syncSongs, .syncConnection, .syncControl, .syncDevice:
            true
        default: false
        }
    }
}

public struct KeptNavigation: Codable, Equatable, Sendable {
    public let tab: Root
    public let paths: [Root: [Destination]]

    public init(tab: Root, paths: [Root: [Destination]]) {
        self.tab = tab
        self.paths = paths
    }

    public var destinations: [Destination] {
        Array(Set(paths.values.joined()))
    }

    public var encoded: Data? {
        try? JSONEncoder().encode(self)
    }

    public static func decoded(_ data: Data) -> KeptNavigation? {
        try? JSONDecoder().decode(KeptNavigation.self, from: data)
    }
}
