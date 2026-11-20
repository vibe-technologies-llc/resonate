import Foundation
import ResonateNative

public enum Root: String, Hashable, Codable, Sendable {
    case home, library, settings, search
}

public enum Destination: Hashable, Codable, Sendable {
    case album(String)
    case artist(String)
    case playlist(UInt64)
    case favourites
    case tools
    case folders
    case suggestions
    case explorePlaylist(String, String)
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
    case playback
    case songLyrics(UInt64?)
    case visualizer
    case songAnalysis(UInt64?)
    case songDetails(UInt64)
    case appearance
    case background
    case identicalSongs
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
        case .sound, .processing, .playback, .output, .scrobbling, .spotify, .providers, .sync, .server, .folders, .appearance,
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
    public let shelf: LibraryShelf?

    public init(tab: Root, paths: [Root: [Destination]], shelf: LibraryShelf? = nil) {
        self.tab = tab
        self.paths = paths
        self.shelf = shelf
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        let tab = try container.decode(Root.self, forKey: .tab)
        let pages = try container.decode([Root: [KeptPage]].self, forKey: .paths)
        let shelf = try container.decodeIfPresent(LibraryShelf.self, forKey: .shelf)
        let carried = CarriedOver(tab: tab, pages: pages, shelf: shelf)

        self.init(tab: carried.tab, paths: carried.paths, shelf: carried.shelf)
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

enum KeptPage: Decodable {
    case shelf(LibraryShelf)
    case page(Destination)

    private struct Named: CodingKey {
        let stringValue: String
        let intValue: Int? = nil

        init?(stringValue: String) {
            self.stringValue = stringValue
        }

        init?(intValue: Int) {
            nil
        }
    }

    init(from decoder: Decoder) throws {
        let names = try decoder.container(keyedBy: Named.self).allKeys
        if names.count == 1, let shelf = LibraryShelf(rawValue: names[0].stringValue) {
            self = .shelf(shelf)
        } else {
            self = .page(try Destination(from: decoder))
        }
    }

    var shelf: LibraryShelf? {
        if case .shelf(let shelf) = self { shelf } else { nil }
    }

    var destination: Destination? {
        if case .page(let destination) = self { destination } else { nil }
    }
}

struct CarriedOver {
    let tab: Root
    let paths: [Root: [Destination]]
    let shelf: LibraryShelf?

    init(tab: Root, pages: [Root: [KeptPage]], shelf: LibraryShelf?) {
        let onTop = pages[tab]?.last?.shelf
        let inTheLibrary = pages[.library]?.compactMap(\.shelf).last

        self.tab = onTop == nil ? tab : .library
        self.shelf = shelf ?? onTop ?? inTheLibrary
        self.paths = pages
            .mapValues { $0.compactMap(\.destination) }
            .filter { !$0.value.isEmpty && (onTop == nil || $0.key != .library) }
    }
}
