import Foundation

public enum SizeClass: Sendable, Equatable {
    case compact
    case regular
}

public struct ScreenShape: Sendable, Equatable {
    public let horizontal: SizeClass
    public let vertical: SizeClass
    public let width: Double
    public let height: Double
    public let accessibilityText: Bool

    public init(horizontal: SizeClass, vertical: SizeClass, width: Double, height: Double, accessibilityText: Bool) {
        self.horizontal = horizontal
        self.vertical = vertical
        self.width = width
        self.height = height
        self.accessibilityText = accessibilityText
    }

    public var isWiderThanTall: Bool {
        width > height
    }
}

public struct PlayerArrangement: Sendable, Equatable {
    public let sideBySide: Bool
    public let scrolls: Bool

    public init(sideBySide: Bool, scrolls: Bool) {
        self.sideBySide = sideBySide
        self.scrolls = scrolls
    }

    public static func of(_ shape: ScreenShape) -> PlayerArrangement {
        let phoneOnItsSide = shape.vertical == .compact && !shape.accessibilityText
        let roomForTwoColumns = shape.horizontal == .regular && shape.vertical == .regular && shape.isWiderThanTall
        let sideBySide = phoneOnItsSide || roomForTwoColumns
        let scrolls = shape.accessibilityText || (sideBySide && shape.vertical == .compact)
        return PlayerArrangement(sideBySide: sideBySide, scrolls: scrolls)
    }
}

public enum LibraryArrangement: Sendable, Equatable {
    case stack
    case split

    public static func of(horizontal: SizeClass, accessibilityText: Bool) -> LibraryArrangement {
        horizontal == .regular && !accessibilityText ? .split : .stack
    }
}

public enum LibrarySection: String, Sendable, Equatable, CaseIterable, Identifiable {
    case home
    case favourites
    case songs
    case playlists
    case albums
    case artists

    public var id: String { rawValue }

    public var title: String {
        switch self {
        case .home: "Library"
        case .favourites: "Favourites"
        case .songs: "All songs"
        case .playlists: "Playlists"
        case .albums: "Albums"
        case .artists: "Artists"
        }
    }

    public var symbol: String {
        switch self {
        case .home: "music.note.house"
        case .favourites: "heart"
        case .songs: "music.note"
        case .playlists: "music.note.list"
        case .albums: "square.stack"
        case .artists: "music.mic"
        }
    }

    public var isHome: Bool {
        self == .home
    }
}

public enum SheetWeight: Sendable, Equatable {
    case short
    case long
}

public enum SheetSizing: Sendable, Equatable {
    case form
    case page

    public static func of(_ weight: SheetWeight, accessibilityText: Bool) -> SheetSizing {
        weight == .long || accessibilityText ? .page : .form
    }
}
