import Foundation
import ResonateNative

public enum HigherStanding: Equatable, Sendable {
    case found
    case chosen
    case waiting
    case none
}

public struct HigherKept: Equatable, Sendable {
    public var chosen: Int
    public var downloading: Int
    public var passedOver: Int

    public init(chosen: Int, downloading: Int, passedOver: Int) {
        self.chosen = chosen
        self.downloading = downloading
        self.passedOver = passedOver
    }
}

public enum HigherVersionRows {
    public static func standing(_ song: HigherVersion) -> HigherStanding {
        if song.pending { return .waiting }
        guard song.found != nil else { return .none }
        return song.chosen ? .chosen : .found
    }

    public static func tickable(_ song: HigherVersion) -> Bool {
        standing(song) == .found
    }

    public static func ticked(_ songs: [HigherVersion]) -> Set<UInt64> {
        Set(songs.filter(tickable).map(\.song.id))
    }

    public static func stillTicked(_ songs: [HigherVersion], _ ticked: Set<UInt64>) -> Set<UInt64> {
        ticked.intersection(Self.ticked(songs))
    }

    public static func picked(_ songs: [HigherVersion], _ ticked: Set<UInt64>) -> [HigherVersionPicked] {
        songs.compactMap { song in
            guard let found = song.found, ticked.contains(song.song.id), tickable(song) else { return nil }
            return HigherVersionPicked(song: song.song.id, version: found.version)
        }
    }

    public static func line(_ song: HigherVersion) -> String {
        let label = song.found?.label ?? ""
        switch standing(song) {
        case .found: return said("higher.found", song.song.codec, label)
        case .chosen: return said("higher.chosen", song.song.codec, label)
        case .waiting: return said("higher.waiting", song.song.codec)
        case .none: return said("higher.none", song.song.codec)
        }
    }

    public static func told(_ done: HigherKept) -> String {
        var parts: [String] = []
        if done.chosen > 0 { parts.append(said("higher.told.chosen", done.chosen)) }
        if done.downloading > 0 { parts.append(said("higher.told.downloading", done.downloading)) }
        if done.passedOver > 0 { parts.append(said("higher.told.passed-over", done.passedOver)) }
        return parts.isEmpty ? said("higher.told.nothing") : parts.joined(separator: said("higher.told.apart"))
    }
}
