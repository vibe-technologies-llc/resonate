import Foundation
import ResonateNative

public enum OnlineMode: String, CaseIterable, Sendable {
    case songs
    case albums
    case artists
}

public struct OnlineAsked: Hashable, Sendable {
    public static let settle: Duration = .milliseconds(450)
    public static let prompt = "Song, album, artist or shared link"
    private static let linkPrefixes = ["http://", "https://", "spotify:"]

    public let text: String
    public let mode: OnlineMode?

    public static func chain(_ text: String) -> [OnlineAsked] {
        if isLink(text) { return [OnlineAsked(text: text, mode: nil)] }
        guard !text.isEmpty else { return [] }
        return OnlineMode.allCases.map { OnlineAsked(text: text, mode: $0) }
    }

    public static func isLink(_ text: String) -> Bool {
        let lowered = text.lowercased()
        return linkPrefixes.contains { lowered.hasPrefix($0) }
    }
}

public enum OnlineAnswer: Sendable {
    case songs([SearchSong])
    case albums([SearchAlbum])
    case artists([SearchArtist])
    case link(OnlineCollection)

    public static func asked(_ asked: OnlineAsked, of native: MobilePlayer) throws -> OnlineAnswer {
        switch asked.mode {
        case nil: .link(try native.openOnlineLink(text: asked.text))
        case .songs: .songs(try native.searchOnline(text: asked.text))
        case .albums: .albums(try native.searchAlbumsOnline(text: asked.text))
        case .artists: .artists(try native.searchArtistsOnline(text: asked.text))
        }
    }
}

public struct OnlineFound: Sendable {
    public private(set) var text = ""
    public private(set) var songs: [SearchSong]?
    public private(set) var albums: [SearchAlbum]?
    public private(set) var artists: [SearchArtist]?
    public private(set) var link: OnlineCollection?

    public init() {}

    public init(text: String, answers: [OnlineAnswer]) {
        self.text = text
        for answer in answers { take(answer) }
    }

    public func landing(_ answer: OnlineAnswer, for text: String) -> OnlineFound {
        var found = self.text == text ? self : OnlineFound(text: text, answers: [])
        found.take(answer)
        return found
    }

    public var isWhole: Bool { link != nil || (songs != nil && albums != nil && artists != nil) }

    public var isEmpty: Bool {
        if let link { return link.songs.isEmpty && link.releases.isEmpty && link.artists.isEmpty }
        return (songs ?? []).isEmpty && (albums ?? []).isEmpty && (artists ?? []).isEmpty
    }

    public func awaited(for asked: String) -> OnlineMode? {
        guard text == asked else { return .songs }
        if songs == nil { return .songs }
        if albums == nil { return .albums }
        return artists == nil ? .artists : nil
    }

    private mutating func take(_ answer: OnlineAnswer) {
        switch answer {
        case .songs(let found): songs = found
        case .albums(let found): albums = found
        case .artists(let found): artists = found
        case .link(let found): link = found
        }
    }
}

public struct OnlineAnswers: Sendable {
    private static let keptPerMode = 64

    private var kept: [OnlineAsked: OnlineAnswer] = [:]
    private var spellings: [OnlineMode?: [String]] = [:]

    public init() {}

    public func answer(for asked: OnlineAsked) -> OnlineAnswer? {
        kept[asked]
    }

    public mutating func keep(_ answer: OnlineAnswer, for asked: OnlineAsked) {
        var recent = spellings[asked.mode, default: []].filter { $0 != asked.text }
        recent.append(asked.text)
        while recent.count > Self.keptPerMode {
            kept[OnlineAsked(text: recent.removeFirst(), mode: asked.mode)] = nil
        }

        spellings[asked.mode] = recent
        kept[asked] = answer
    }
}
