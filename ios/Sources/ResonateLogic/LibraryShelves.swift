import ResonateNative

public enum LibraryShelf: String, Codable, CaseIterable, Identifiable, Sendable {
    case songs
    case albums
    case artists
    case playlists

    public var id: String { rawValue }

    public var title: String {
        switch self {
        case .songs: said("library.shelf.songs")
        case .albums: said("library.shelf.albums")
        case .artists: said("library.shelf.artists")
        case .playlists: said("library.shelf.playlists")
        }
    }

    public var symbol: String {
        switch self {
        case .songs: "music.note"
        case .albums: "square.stack"
        case .artists: "music.mic"
        case .playlists: "music.note.list"
        }
    }

    public var orders: [CollectionOrder] {
        switch self {
        case .albums: [.alphabetical, .artist, .year, .added, .plays]
        case .artists: [.alphabetical, .added, .plays]
        case .songs, .playlists: []
        }
    }
}

public enum LibraryOpening: Equatable, Sendable {
    case songs
    case albums(by: CollectionOrder?)
    case artists(by: CollectionOrder?)
    case playlists

    public var shelf: LibraryShelf {
        switch self {
        case .songs: .songs
        case .albums: .albums
        case .artists: .artists
        case .playlists: .playlists
        }
    }

    public var albumOrder: CollectionOrder? {
        guard case .albums(let order) = self else { return nil }
        return order.flatMap(offered)
    }

    public var artistOrder: CollectionOrder? {
        guard case .artists(let order) = self else { return nil }
        return order.flatMap(offered)
    }

    public var clearsFilters: Bool {
        self == .songs
    }

    private func offered(_ order: CollectionOrder) -> CollectionOrder? {
        shelf.orders.contains(order) ? order : nil
    }
}

public enum AlbumsLayout: Equatable, Sendable {
    case grid
    case rows

    public static func shown(asRows: Bool, accessibilityText: Bool) -> AlbumsLayout {
        asRows || accessibilityText ? .rows : .grid
    }
}
