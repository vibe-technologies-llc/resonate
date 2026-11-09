import Foundation

public enum SizeClass: Sendable, Equatable {
    case compact
    case regular
}

public struct Fold: Sendable, Equatable {
    public enum Runs: Sendable, Equatable {
        case down
        case across
    }

    public let runs: Runs
    public let start: Double
    public let end: Double

    public init(runs: Runs, start: Double, end: Double) {
        self.runs = runs
        self.start = start
        self.end = end
    }
}

public struct ScreenShape: Sendable, Equatable {
    public let horizontal: SizeClass
    public let vertical: SizeClass
    public let width: Double
    public let height: Double
    public let accessibilityText: Bool
    public let fold: Fold?

    public init(
        horizontal: SizeClass,
        vertical: SizeClass,
        width: Double,
        height: Double,
        accessibilityText: Bool,
        fold: Fold? = nil
    ) {
        self.horizontal = horizontal
        self.vertical = vertical
        self.width = width
        self.height = height
        self.accessibilityText = accessibilityText
        self.fold = fold
    }

    public var isWiderThanTall: Bool {
        width > height
    }
}

public enum PlayerSeam: Sendable, Equatable {
    case none
    case beside(leading: Double, gap: Double, trailing: Double)
    case stacked(top: Double, gap: Double, bottom: Double)

    public static let narrowestSide: Double = 240

    public static func of(_ shape: ScreenShape) -> PlayerSeam {
        guard let fold = shape.fold, fold.start < fold.end else { return .none }
        let length = fold.runs == .down ? shape.width : shape.height
        let before = fold.start
        let after = length - fold.end
        guard before >= narrowestSide, after >= narrowestSide else { return .none }
        let gap = fold.end - fold.start
        return switch fold.runs {
        case .down: .beside(leading: before, gap: gap, trailing: after)
        case .across: .stacked(top: before, gap: gap, bottom: after)
        }
    }
}

public struct PlayerArrangement: Sendable, Equatable {
    public let sideBySide: Bool
    public let scrolls: Bool
    public let seam: PlayerSeam

    public init(sideBySide: Bool, scrolls: Bool, seam: PlayerSeam = .none) {
        self.sideBySide = sideBySide
        self.scrolls = scrolls
        self.seam = seam
    }

    public var stackedAtTheFold: Bool {
        if case .stacked = seam { true } else { false }
    }

    public static func of(_ shape: ScreenShape) -> PlayerArrangement {
        let seam = PlayerSeam.of(shape)
        switch seam {
        case .beside: return PlayerArrangement(sideBySide: true, scrolls: shape.accessibilityText, seam: seam)
        case .stacked: return PlayerArrangement(sideBySide: false, scrolls: shape.accessibilityText, seam: seam)
        case .none: break
        }
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

public enum EqualiserGraphShape {
    public static let heightOverWidth = 10.0 / 16.0
    public static let shortest = 180.0
    public static let tallest = 340.0

    public static func height(forWidth width: Double) -> Double {
        min(max(width * heightOverWidth, shortest), tallest)
    }
}
