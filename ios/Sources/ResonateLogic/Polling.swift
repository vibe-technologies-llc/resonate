import Foundation
import ResonateNative

public struct QueueListed: Equatable {
    public static let catalogRereadAfter = Duration.seconds(3)

    public let zone: UInt32
    public let queue: UInt64
    public let catalog: UInt64

    public init(_ state: Snapshot) {
        zone = state.zone
        queue = state.queueRevision
        catalog = state.catalogRevision
    }

    public func isDue(after read: QueueListed?, since readAt: ContinuousClock.Instant) -> Bool {
        guard let read else { return true }
        if zone != read.zone || queue != read.queue { return true }
        return catalog != read.catalog && readAt.duration(to: .now) >= Self.catalogRereadAfter
    }
}

public struct SteeredSong: Equatable {
    public let track: Track
    public let index: Int

    public init(track: Track, index: Int) {
        self.track = track
        self.index = index
    }
}

public struct Steered {
    public var transport: Transport?
    public var shuffle: Bool?
    public var repeatMode: UInt32?
    public var song: SteeredSong?
    public var restarted = false

    public init() {}

    public func applied(to state: Snapshot) -> Snapshot {
        var steered = state
        if let transport { steered.transport = transport }
        if let shuffle { steered.shuffle = shuffle }
        if let repeatMode { steered.repeat = repeatMode }
        if let song {
            steered.current = song.track
            steered.queueIndex = UInt32(song.index)
            steered.positionMs = 0
            steered.durationMs = song.track.durationMs
        } else if restarted {
            steered.positionMs = 0
        }
        return steered
    }
}

public struct Settling {
    public private(set) var asked = 0
    public private(set) var answered = 0

    public init() {}

    public var settled: Bool { asked == answered }

    public mutating func ask() {
        asked += 1
    }

    public mutating func answer() {
        answered += 1
    }
}

public enum Polling {
    public static let busy: Duration = .milliseconds(250)
    public static let nearEnd: Duration = .milliseconds(100)
    public static let idle: Duration = .seconds(1)
    public static let background: Duration = .seconds(2)
    public static let nearTheEndMs: UInt64 = 1500

    public static func interval(foreground: Bool, state: Snapshot, analysing: Bool, asked: Bool) -> Duration {
        if asked { return busy }
        let moving = state.transport == .playing || state.transport == .loading
        switch (foreground, moving) {
        case (true, true): return endingSoon(state) ? nearEnd : busy
        case (true, false): return analysing ? busy : idle
        case (false, _): return background
        }
    }

    public static func endingSoon(_ state: Snapshot) -> Bool {
        state.transport == .playing && state.durationMs > state.positionMs
            && state.durationMs - state.positionMs <= nearTheEndMs
    }

    public static func steady(_ state: Snapshot) -> Snapshot {
        var steady = state
        steady.positionMs = 0
        steady.durationMs = 0
        steady.sleepRemainingMs = state.sleepRemainingMs.map { _ in 0 }
        steady.outputRate = 0
        steady.underruns = 0
        steady.overs = 0
        steady.failure = nil
        steady.queueRevision = 0
        steady.catalogRevision = 0
        return steady
    }
}

public enum Steering {
    public static let previousRestartsPastMs: UInt64 = 3000

    public static func previousRestarts(positionMs: UInt64, durationMs: UInt64) -> Bool {
        positionMs > previousRestartsPastMs && durationMs > previousRestartsPastMs
    }

    public static func neighbour(of playing: Int?, step: Int, in queue: [Track], repeating: Bool) -> SteeredSong? {
        guard let playing, !queue.isEmpty else { return nil }
        let at = playing + step
        if queue.indices.contains(at) { return SteeredSong(track: queue[at], index: at) }
        guard step > 0, repeating else { return nil }
        return SteeredSong(track: queue[0], index: 0)
    }
}
