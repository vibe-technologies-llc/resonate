import Foundation
import ResonateNative

public enum HomeGreeting {
    public static func of(hour: Int) -> String {
        switch hour {
        case 5..<12: said("home.greeting.morning")
        case 12..<18: said("home.greeting.afternoon")
        default: said("home.greeting.evening")
        }
    }
}

public struct JumpBackTile: Identifiable, Hashable, Sendable {
    public enum Subject: Hashable, Sendable {
        case album(key: String, artist: String)
        case favourites
    }

    public let subject: Subject
    public let title: String
    public let detail: String
    public let cover: Track?

    public var id: Subject { subject }

    public var destination: Destination {
        switch subject {
        case .album(let key, _): .album(key)
        case .favourites: .favourites
        }
    }

    public var spoken: String {
        detail.isEmpty ? title : said("home.jump-back-in.spoken", title, detail)
    }

    public static var favourites: JumpBackTile {
        JumpBackTile(subject: .favourites, title: said("favourites.title"), detail: "", cover: nil)
    }
}

public enum JumpBackIn {
    public static let songsRead: UInt32 = 100
    public static let albumsKept = 8
    public static let fewestShown = 2

    public static var heardLately: TrackFilter {
        var filter = defaultTrackFilter()
        filter.played = true
        filter.order = .played
        filter.descending = true
        return filter
    }

    public static func albums(heard songs: [Track], keeping: Int = albumsKept) -> [JumpBackTile] {
        var seen = Set<String>()
        var tiles: [JumpBackTile] = []
        for song in songs where tiles.count < keeping {
            guard !song.album.isEmpty,
                let key = albumKey(album: song.album, artist: song.artist, albumArtist: song.albumArtist),
                seen.insert(key).inserted
            else { continue }
            tiles.append(
                JumpBackTile(
                    subject: .album(key: key, artist: song.groupingArtist),
                    title: song.album,
                    detail: song.groupingArtist,
                    cover: song
                )
            )
        }
        return tiles
    }

    public static func shown(_ albums: [JumpBackTile], favourites: Bool, columns: Int) -> [JumpBackTile] {
        let places = JumpBackGrid.tiles(columns: columns)
        let tiles = favourites ? Array(albums.prefix(places - 1)) + [.favourites] : Array(albums.prefix(places))
        return tiles.count < fewestShown ? [] : tiles
    }
}

public enum JumpBackGrid {
    public static func columns(horizontal: SizeClass, accessibilityText: Bool) -> Int {
        if accessibilityText { return 1 }
        return horizontal == .regular ? 4 : 2
    }

    public static func tiles(columns: Int) -> Int {
        switch columns {
        case ...1: 4
        case 2, 3: 6
        default: 8
        }
    }
}

public struct HomeMoment: Equatable, Sendable {
    public let catalogRevision: UInt64
    public let day: Date

    public static func of(catalogRevision: UInt64, now: Date = .now, calendar: Calendar = .current) -> HomeMoment {
        HomeMoment(catalogRevision: catalogRevision, day: calendar.startOfDay(for: now))
    }
}
