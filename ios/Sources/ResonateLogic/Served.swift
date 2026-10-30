import Foundation
import ResonateNative

public enum PlayedInPlace {
    public static let schemes = ["subsonic", "tidal", "resonate", "hifi-api", "monochrome"]

    public static func scheme(of uri: String) -> String? {
        guard let colon = uri.firstIndex(of: ":") else { return nil }
        let scheme = String(uri[..<colon])
        return schemes.contains(scheme) ? scheme : nil
    }
}

extension Track {
    public var isServed: Bool { id == 0 && PlayedInPlace.scheme(of: uri) != nil }

    public var pictureKey: String { isServed ? uri : String(id) }
}

extension DownloadAccount {
    public var isHosted: Bool {
        (provider == .monochrome || provider == .hifiApi) && !usesItsOwnServer
    }
}

extension ProviderAccounts {
    public func served(online: Bool, hosted: Bool) -> [DownloadAccount] {
        guard online else { return [] }
        return asked.filter { hosted || !$0.isHosted }
    }
}

public struct ServedChoice: Equatable, Sendable {
    public private(set) var services: [ServedService] = []
    public private(set) var chosen: String?

    public init(services: [ServedService] = []) {
        offer(services)
    }

    public var choosing: Bool { services.count > 1 }

    public func keepsFavourites(of track: Track) -> Bool {
        services.contains { $0.scheme == PlayedInPlace.scheme(of: track.uri) && $0.favourites }
    }

    public var service: ServedService? { services.first { $0.scheme == chosen } }

    public mutating func offer(_ offered: [ServedService]) {
        services = offered
        if !offered.contains(where: { $0.scheme == chosen }) {
            chosen = offered.first?.scheme
        }
    }

    public mutating func choose(_ scheme: String) {
        guard services.contains(where: { $0.scheme == scheme }) else { return }
        chosen = scheme
    }
}

public struct ServedPlace: Hashable, Codable, Sendable {
    public enum Kind: String, Codable, Sendable {
        case artist, album
    }

    public let kind: Kind
    public let uri: String
    public let name: String
    public let artist: String
    public let cover: String?

    public init(kind: Kind, uri: String, name: String, artist: String = "", cover: String? = nil) {
        self.kind = kind
        self.uri = uri
        self.name = name
        self.artist = artist
        self.cover = cover
    }

    public init(_ artist: ServedArtist) {
        self.init(kind: .artist, uri: artist.uri, name: artist.shownName, cover: artist.cover)
    }

    public init(_ album: ServedAlbum) {
        self.init(kind: .album, uri: album.uri, name: album.shownName, artist: album.artist ?? "", cover: album.cover)
    }
}

extension ServedArtist {
    public var shownName: String { name.isEmpty ? "Unknown artist" : name }

    public var line: String? { albums.map { counted(Int($0), "album") } }
}

extension ServedAlbum {
    public var shownName: String { name.isEmpty ? "Unknown album" : name }

    public var line: String {
        [
            artist.flatMap { $0.trimmingCharacters(in: .whitespaces).isEmpty ? nil : $0 },
            year.map { String($0) },
            songs.map { counted(Int($0), "song") },
        ]
        .compactMap { $0 }
        .joined(separator: " · ")
    }
}

extension ServedSong {
    public var shownTitle: String { title.isEmpty ? "Unknown song" : title }

    public func line(withAlbum: Bool) -> String {
        [
            artist.flatMap { $0.trimmingCharacters(in: .whitespaces).isEmpty ? nil : $0 },
            album.flatMap { withAlbum && !$0.isEmpty ? $0 : nil },
            durationMs.map { clock($0) },
        ]
        .compactMap { $0 }
        .joined(separator: " · ")
    }

    public func number(at index: Int) -> UInt32 { track ?? UInt32(index + 1) }
}

extension ServedFound {
    public var isEmpty: Bool { artists.isEmpty && albums.isEmpty && songs.isEmpty }

    public var summary: String {
        [
            artists.isEmpty ? nil : counted(artists.count, "artist"),
            albums.isEmpty ? nil : counted(albums.count, "album"),
            songs.isEmpty ? nil : counted(songs.count, "song"),
        ]
        .compactMap { $0 }
        .joined(separator: " · ")
    }
}

public func servedSummary(of songs: [ServedSong]) -> String {
    let lengths = songs.compactMap(\.durationMs)
    guard !lengths.isEmpty else { return counted(songs.count, "song") }
    let minutes = lengths.reduce(0, +) / 60_000
    let length = minutes >= 60 ? "\(minutes / 60) hr \(minutes % 60) min" : "\(minutes) min"
    return "\(counted(songs.count, "song")) · \(length)"
}
