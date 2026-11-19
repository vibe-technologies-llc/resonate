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

public struct ShelfRowDrag: Equatable, Sendable {
    public let width: Double
    public let count: Int
    public let between: Double

    public init(width: Double, count: Int, between: Double) {
        self.width = width
        self.count = count
        self.between = between
    }

    public var segment: Double {
        guard count > 0 else { return 0 }
        return max((width - between * Double(count - 1)) / Double(count), 0)
    }

    public func centre(of index: Int) -> Double {
        Double(index) * (segment + between) + segment / 2
    }

    public func clamped(_ x: Double) -> Double {
        guard count > 0 else { return x }
        return min(max(x, centre(of: 0)), centre(of: count - 1))
    }

    public func nearest(to x: Double) -> Int {
        let pitch = segment + between
        guard count > 0, pitch > 0 else { return 0 }
        let place = Int(((x - segment / 2) / pitch).rounded())
        return min(max(place, 0), count - 1)
    }
}

public struct SearchReveal: Equatable, Sendable {
    public static let turnsAfter: Double = 24

    public private(set) var shown = false
    private var last: Double?
    private var travelled: Double = 0

    public init() {}

    public mutating func scrolled(to offset: Double, within range: Double) {
        let place = min(offset, max(range, 0))
        defer { last = place }
        guard let last else { return }
        let moved = place >= last ? max(place, 0) - max(last, 0) : place - last
        guard moved != 0 else { return }
        if travelled != 0, (moved > 0) != (travelled > 0) { travelled = 0 }
        travelled += moved
        if travelled >= Self.turnsAfter {
            shown = false
        } else if travelled <= -Self.turnsAfter {
            shown = true
        }
    }

    public mutating func startOver() {
        shown = false
        last = nil
        travelled = 0
    }
}
