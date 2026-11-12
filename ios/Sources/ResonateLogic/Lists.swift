import ResonateNative

public struct PlaylistEntry: Identifiable, Equatable, Sendable {
    public struct Key: Hashable, Sendable {
        public let song: UInt64
        public let occurrence: Int
    }

    public let id: Key
    public let position: Int
    public let track: Track

    public static func of(_ rows: [Track]) -> [PlaylistEntry] {
        var seen: [UInt64: Int] = [:]
        return rows.enumerated().map { position, track in
            let occurrence = seen[track.id, default: 0]
            seen[track.id] = occurrence + 1
            return PlaylistEntry(id: Key(song: track.id, occurrence: occurrence), position: position, track: track)
        }
    }
}

public struct SongKeys: Sendable {
    public let tracks: [Track]
    public let keys: [String]

    public static let empty = SongKeys(tracks: [], keys: [])

    public static func of(_ tracks: [Track]) -> SongKeys {
        SongKeys(tracks: tracks, keys: tracks.map { "\($0.shownTitle)\u{1f}\($0.artist)\u{1f}\($0.album)".searchFolded })
    }

    public func matching(_ folded: String) -> [Track] {
        guard !folded.isEmpty else { return tracks }
        return zip(tracks, keys).filter { $0.1.contains(folded) }.map(\.0)
    }
}

public enum AlbumRow: Identifiable, Sendable {
    case held(Track)
    case notHeld(SearchSong)

    public var id: String {
        switch self {
        case .held(let track): "held:\(track.id)"
        case .notHeld(let song): "not-held:\(song.disc ?? 1):\(song.position ?? 0):\(song.id)"
        }
    }

    var place: UInt32? {
        switch self {
        case .held(let track): track.trackNumber
        case .notHeld(let song): song.position
        }
    }
}

public struct AlbumDisc: Identifiable, Sendable {
    public let number: UInt32
    public let run: Int
    public var rows: [AlbumRow]

    public var id: Int { run }

    public var tracks: [Track] {
        rows.compactMap { row in
            if case .held(let track) = row { track } else { nil }
        }
    }

    public static func of(_ tracks: [Track], notHeld: [SearchSong] = []) -> [AlbumDisc] {
        var discs: [AlbumDisc] = []
        for track in tracks {
            let number = track.discNumber ?? 1
            if discs.last?.number == number {
                discs[discs.count - 1].rows.append(.held(track))
            } else {
                discs.append(AlbumDisc(number: number, run: discs.count, rows: [.held(track)]))
            }
        }
        var missingIds = Set<String>()
        for song in notHeld {
            guard missingIds.insert(AlbumRow.notHeld(song).id).inserted else { continue }
            if let position = song.position, tracks.contains(where: {
                ($0.discNumber ?? 1) == (song.disc ?? 1) && $0.trackNumber == position
            }) { continue }
            let number = song.disc ?? 1
            if let found = discs.firstIndex(where: { $0.number == number }) {
                discs[found].lay(song)
            } else {
                let after = discs.firstIndex { $0.number > number } ?? discs.endIndex
                discs.insert(AlbumDisc(number: number, run: 0, rows: [.notHeld(song)]), at: after)
            }
        }
        return discs.enumerated().map { AlbumDisc(number: $1.number, run: $0, rows: $1.rows) }
    }

    private mutating func lay(_ song: SearchSong) {
        let place = song.position ?? .max
        let before = rows.firstIndex { ($0.place ?? 0) > place } ?? rows.endIndex
        rows.insert(.notHeld(song), at: before)
    }
}

public func sharedQuality(_ tracks: [Track]) -> String? {
    guard let first = tracks.first,
        tracks.allSatisfy({ $0.codec == first.codec && $0.bits == first.bits && $0.rate == first.rate })
    else { return nil }
    let quality = first.quality
    return quality.isEmpty ? nil : quality
}

public struct RowWindow: Equatable, Sendable {
    public static let opening = 300
    public static let widenedBy = 300

    public private(set) var atMost = RowWindow.opening

    public init() {}

    public func shown(of rows: Int) -> Int {
        min(rows, atMost)
    }

    public func holdsMore(than rows: Int) -> Bool {
        rows > atMost
    }

    public mutating func widen() {
        atMost += Self.widenedBy
    }
}

public struct QueueParts: Sendable {
    public let history: Range<Int>
    public let next: Range<Int>
    public let rest: Range<Int>
    public let beyond: Int

    public init(rows: Int, playing: Int, next asked: Range<Int>?, heardAtMost: Int? = nil, aheadAtMost: Int? = nil) {
        let after = min(playing + 1, rows)
        let heard = min(playing, rows)
        let shownTo = aheadAtMost.map { min(after + $0, rows) } ?? rows
        history = (heardAtMost.map { max(heard - $0, 0) } ?? 0)..<heard
        let nextEnd = asked.map { min(max($0.upperBound, after), shownTo) } ?? after
        next = after..<nextEnd
        rest = nextEnd..<max(shownTo, nextEnd)
        beyond = rows - max(shownTo, nextEnd)
    }
}

public struct NextPlacement: Sendable {
    private let playing: Int
    private let next: Range<Int>

    public init(playing: Int, next: Range<Int>) {
        self.playing = playing
        self.next = next
    }

    public func takes(_ index: Int) -> Int? {
        guard index != playing, !next.contains(index), index != next.upperBound else { return nil }
        return next.upperBound
    }
}

public struct SongOrdering: Equatable, Hashable, Sendable {
    public var order: TrackOrder
    public var descending: Bool

    public init(order: TrackOrder, descending: Bool) {
        self.order = order
        self.descending = descending
    }

    public static let newestFirst = SongOrdering(order: .added, descending: true)

    public func favourites() -> TrackFilter {
        var filter = defaultTrackFilter()
        filter.favourites = true
        filter.order = order
        filter.descending = descending
        return filter
    }
}

public enum SongBrowsing {
    public static var defaultFilter: TrackFilter {
        var filter = defaultTrackFilter()
        filter.order = .added
        filter.descending = true
        return filter
    }
}

public enum FilterChip: Identifiable, Sendable {
    case favourites
    case unplayed
    case played
    case hiRes
    case depth(UInt32)
    case genre(String)
    case artist(String)
    case album(String)
    case order(TrackOrder, descending: Bool)

    public static var orderNames: [(TrackOrder, String)] {
        [
            (.title, said("filter.order.title")),
            (.album, said("filter.order.album")),
            (.artist, said("filter.order.artist")),
            (.duration, said("filter.order.duration")),
            (.added, said("filter.order.added")),
            (.plays, said("filter.order.plays")),
            (.played, said("filter.order.played")),
        ]
    }

    static func sortedBy(_ order: TrackOrder) -> String {
        switch order {
        case .title: said("filter.chip.sorted.title")
        case .album: said("filter.chip.sorted.album")
        case .artist: said("filter.chip.sorted.artist")
        case .duration: said("filter.chip.sorted.duration")
        case .added: said("filter.chip.sorted.added")
        case .plays: said("filter.chip.sorted.plays")
        case .played: said("filter.chip.sorted.played")
        }
    }

    public static func of(_ filter: TrackFilter) -> [FilterChip] {
        var chips: [FilterChip] = []
        if filter.favourites { chips.append(.favourites) }
        if filter.unplayed { chips.append(.unplayed) }
        if filter.played { chips.append(.played) }
        if filter.minimumRate > 0 { chips.append(.hiRes) }
        if filter.minimumBits > 0 { chips.append(.depth(filter.minimumBits)) }
        if !filter.genre.isEmpty { chips.append(.genre(filter.genre)) }
        if !filter.artist.isEmpty { chips.append(.artist(filter.artist)) }
        if !filter.album.isEmpty { chips.append(.album(filter.album)) }
        if filter.order != .added || !filter.descending { chips.append(.order(filter.order, descending: filter.descending)) }
        return chips
    }

    public var id: String { label }

    public var label: String {
        switch self {
        case .favourites: said("filter.chip.favourites")
        case .unplayed: said("filter.chip.unplayed")
        case .played: said("filter.chip.played")
        case .hiRes: said("filter.chip.hi-res")
        case .depth(let bits): said("filter.chip.depth", Int(bits))
        case .genre(let genre): said("filter.chip.genre", genre)
        case .artist(let artist): said("filter.chip.artist", artist)
        case .album(let album): said("filter.chip.album", album)
        case .order(let order, let descending):
            descending ? said("order.descending", Self.sortedBy(order)) : Self.sortedBy(order)
        }
    }

    public func cleared(from filter: TrackFilter) -> TrackFilter {
        var cleared = filter
        switch self {
        case .favourites: cleared.favourites = false
        case .unplayed: cleared.unplayed = false
        case .played: cleared.played = false
        case .hiRes: cleared.minimumRate = 0
        case .depth: cleared.minimumBits = 0
        case .genre: cleared.genre = ""
        case .artist: cleared.artist = ""
        case .album: cleared.album = ""
        case .order:
            cleared.order = .added
            cleared.descending = true
        }
        return cleared
    }
}

public enum NotInYourLibrary {
    public static let songsShownFirst = 20

    public enum Lacked: Sendable {
        case release
        case song
    }

    public static func heading(_ count: Int, _ lacked: Lacked) -> String {
        switch lacked {
        case .release: said("not-in-library.heading.releases", count)
        case .song: said("not-in-library.heading.songs", count)
        }
    }

    public static func shown(_ songs: [SearchSong], all: Bool) -> [SearchSong] {
        all ? songs : Array(songs.prefix(songsShownFirst))
    }

    public static func line(_ release: ReleaseNotHeld) -> String {
        [release.kind, release.year.map(String.init)].compactMap { $0 }.filter { !$0.isEmpty }.joined(separator: " · ")
    }
}

public enum QueueShape: Equatable, Sendable {
    case empty
    case one
    case several

    public init(count: Int) {
        switch count {
        case 0: self = .empty
        case 1: self = .one
        default: self = .several
        }
    }
}
