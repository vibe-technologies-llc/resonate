import Foundation
import ResonateNative

public enum LossyStanding: Equatable, Sendable {
    case held
    case wantable
    case waiting
    case unmatched
}

public struct LossyReplaced: Equatable, Sendable {
    public var replaced: Int
    public var wanted: Int
    public var unmatched: Int
    public var kept: Int

    public init(replaced: Int, wanted: Int, unmatched: Int, kept: Int) {
        self.replaced = replaced
        self.wanted = wanted
        self.unmatched = unmatched
        self.kept = kept
    }
}

public enum LossySongs {
    public static func standing(_ song: LossySong) -> LossyStanding {
        if song.lossless != nil { return .held }
        if song.pending { return .waiting }
        return song.wantable ? .wantable : .unmatched
    }

    public static func tickable(_ song: LossySong) -> Bool {
        switch standing(song) {
        case .held, .wantable: true
        case .waiting, .unmatched: false
        }
    }

    public static func ticked(_ songs: [LossySong]) -> Set<UInt64> {
        Set(songs.filter(tickable).map(\.song.id))
    }

    public static func stillTicked(_ songs: [LossySong], _ ticked: Set<UInt64>) -> Set<UInt64> {
        ticked.intersection(Self.ticked(songs))
    }

    public static func line(_ song: LossySong) -> String {
        switch standing(song) {
        case .held: said("lossy.held", song.song.codec, song.lossless?.codec ?? "")
        case .wantable: said("lossy.wantable", song.song.codec)
        case .waiting: said("lossy.waiting", song.song.codec)
        case .unmatched: said("lossy.unmatched", song.song.codec)
        }
    }

    public static func deletes(_ songs: [LossySong], _ ticked: Set<UInt64>) -> Int {
        songs.filter { ticked.contains($0.song.id) && standing($0) == .held }.count
    }

    public static func told(_ done: LossyReplaced) -> String {
        var parts: [String] = []
        if done.replaced > 0 { parts.append(said("lossy.told.replaced", done.replaced)) }
        if done.wanted > 0 { parts.append(said("lossy.told.wanted", done.wanted)) }
        if done.unmatched > 0 { parts.append(said("lossy.told.unmatched", done.unmatched)) }
        if done.kept > 0 { parts.append(said("lossy.told.kept", done.kept)) }
        return parts.isEmpty ? said("lossy.told.nothing") : parts.joined(separator: said("lossy.told.apart"))
    }
}
