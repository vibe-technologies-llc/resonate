import Foundation
import ResonateNative

public enum RecompressTo: String, CaseIterable, Identifiable, Sendable {
    case lossless
    case studio
    case high
    case cd

    public static let key = "recompress-to"
    public static let standing: RecompressTo = .lossless

    public var id: String { rawValue }

    public static func kept(_ stored: String?) -> RecompressTo {
        stored.flatMap(RecompressTo.init(rawValue:)) ?? standing
    }

    public var ceiling: RecompressCeiling {
        switch self {
        case .lossless: .kept
        case .studio: .studio
        case .high: .high
        case .cd: .cd
        }
    }

    public var lowersTheResolution: Bool {
        self != .lossless
    }

    public var title: String {
        switch self {
        case .lossless: said("storage.ceiling.lossless")
        case .studio: said("storage.ceiling.studio")
        case .high: said("storage.ceiling.high")
        case .cd: said("storage.ceiling.cd")
        }
    }
}

public struct StoragePages: Equatable, Sendable {
    public static let pageRows: UInt32 = 100

    public private(set) var songs: [SizedSong] = []
    public private(set) var after: SizedSongAfter?
    public private(set) var ended = false

    public init() {}

    public var listedBytes: UInt64 {
        songs.reduce(0) { $0 + $1.bytes }
    }

    public static func comesFirst(_ one: SizedSong, _ other: SizedSong) -> Bool {
        one.bytes != other.bytes ? one.bytes > other.bytes : one.track.id < other.track.id
    }

    static func lies(_ song: SizedSong, beyond after: SizedSongAfter) -> Bool {
        song.bytes < after.bytes || (song.bytes == after.bytes && song.track.id > after.id)
    }

    public mutating func took(_ page: [SizedSong]) {
        let fresh = Set(page.map(\.track.id))
        songs.removeAll { fresh.contains($0.track.id) }
        songs = (songs + page).sorted(by: Self.comesFirst)
        if let last = page.last {
            after = SizedSongAfter(bytes: last.bytes, id: last.track.id)
        }
        ended = page.count < Int(Self.pageRows)
    }

    public mutating func resized(_ track: Track, bytes: UInt64) {
        guard let at = songs.firstIndex(where: { $0.track.id == track.id }) else { return }
        songs.remove(at: at)
        let moved = SizedSong(track: track, bytes: bytes)
        if !ended, let after, Self.lies(moved, beyond: after) { return }
        songs = (songs + [moved]).sorted(by: Self.comesFirst)
    }

    public mutating func retain(_ held: (UInt64) -> Bool) {
        songs.removeAll { !held($0.track.id) }
    }
}

public enum StorageRows {
    public static func recompressible(_ track: Track) -> Bool {
        track.codec.caseInsensitiveCompare("FLAC") == .orderedSame
    }

    public static func format(_ track: Track) -> String {
        var parts = [track.codec]
        if let bits = track.bits { parts.append(said("player.quality.bits", Int(bits))) }
        if track.rate > 0 { parts.append(said("player.quality.rate", String(format: "%g", Double(track.rate) / 1000))) }
        return parts.filter { !$0.isEmpty }.joined(separator: " · ")
    }

    public static func line(_ song: SizedSong, size: (UInt64) -> String) -> String {
        [song.track.artist, size(song.bytes), format(song.track)].filter { !$0.isEmpty }.joined(separator: " · ")
    }
}

public enum RecompressOutcome: Equatable, Sendable {
    case shrunk(before: UInt64, after: UInt64)
    case kept(String)
    case failed(String)

    public func told(title: String, size: (UInt64) -> String) -> String {
        switch self {
        case .shrunk(let before, let after): said("storage.recompressed", title, size(before), size(after))
        case .kept(let reason): said("storage.recompress.kept", title, reason)
        case .failed(let reason): said("storage.recompress.failed", title, reason)
        }
    }
}

public struct RecompressTally: Equatable, Sendable {
    public private(set) var shrunk = 0
    public private(set) var saved: UInt64 = 0
    public private(set) var kept = 0
    public private(set) var failed = 0
    public private(set) var firstFailure: String?

    public init() {}

    public var done: Int {
        shrunk + kept + failed
    }

    public mutating func add(_ outcome: RecompressOutcome) {
        switch outcome {
        case .shrunk(let before, let after):
            shrunk += 1
            saved += before > after ? before - after : 0
        case .kept:
            kept += 1
        case .failed(let reason):
            failed += 1
            firstFailure = firstFailure ?? reason
        }
    }

    public func told(stopped: Bool, size: (UInt64) -> String) -> String {
        var parts: [String] = []
        if shrunk > 0 { parts.append(said("storage.told.shrunk", shrunk, size(saved))) }
        if kept > 0 { parts.append(said("storage.told.kept", kept)) }
        if failed > 0 { parts.append(said("storage.told.failed", failed)) }
        let tally = parts.isEmpty ? said("storage.told.nothing") : parts.joined(separator: said("storage.told.apart"))
        return stopped ? said("storage.told.stopped", tally) : tally
    }
}
