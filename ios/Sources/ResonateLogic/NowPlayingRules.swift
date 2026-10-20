import Foundation
import ResonateNative

public enum SessionInterruption: Equatable, Sendable {
    case began
    case ended(shouldResume: Bool)
}

public enum SessionAction: Equatable, Sendable {
    case nothing
    case pause
    case resume
}

public struct InterruptionPolicy: Sendable {
    private var resumeAfter = false

    public init() {}

    public mutating func heard(_ interruption: SessionInterruption?, transport: Transport) -> SessionAction {
        switch interruption {
        case .began?:
            resumeAfter = transport == .playing || transport == .loading
            return .pause
        case .ended(let shouldResume)?:
            defer { resumeAfter = false }
            return resumeAfter && shouldResume ? .resume : .nothing
        case nil:
            return .nothing
        }
    }

    public mutating func lostTheDevice() -> SessionAction {
        resumeAfter = false
        return .pause
    }

    public var resumesAfterwards: Bool { resumeAfter }
}

public struct SessionRelease: Sendable {
    private var lastTransport: Transport?
    private var wasSleeping = false

    public init() {}

    public mutating func owed(by snapshot: Snapshot, listening: Bool) -> Bool {
        let sleeping = snapshot.sleepRemainingMs != nil
        let stopped = snapshot.transport == .stopped && lastTransport != nil && lastTransport != .stopped
        let sleptOff = snapshot.transport == .paused && lastTransport == .playing && wasSleeping && !sleeping
        lastTransport = snapshot.transport
        wasSleeping = sleeping
        return (stopped || sleptOff) && !listening
    }
}

public struct CommandAvailability: Equatable, Sendable {
    public let loaded: Bool
    public let next: Bool

    public init(_ shown: NowPlayingShown?) {
        loaded = shown != nil
        let lastRow = (shown?.queueIndex ?? 0) + 1 >= (shown?.queueCount ?? 0)
        let wraps = shown?.repeatMode == NowPlayingShown.repeatsAll
        next = loaded && (!lastRow || wraps)
    }
}

public struct NowPlayingShown: Equatable, Sendable {
    public let track: Track
    public let transport: Transport
    public let durationMs: UInt64
    public let queueIndex: Int
    public let queueCount: Int
    public let shuffle: Bool
    public let repeatMode: UInt32
    public let artRevision: UInt64
    public let artworkReady: Bool

    public init(
        track: Track,
        transport: Transport,
        durationMs: UInt64,
        queueIndex: Int,
        queueCount: Int,
        shuffle: Bool,
        repeatMode: UInt32,
        artRevision: UInt64,
        artworkReady: Bool
    ) {
        self.track = track
        self.transport = transport
        self.durationMs = durationMs
        self.queueIndex = queueIndex
        self.queueCount = queueCount
        self.shuffle = shuffle
        self.repeatMode = repeatMode
        self.artRevision = artRevision
        self.artworkReady = artworkReady
    }

    public static let driftBeforeRewriteMs: Double = 1500
    public static let repeatsAll: UInt32 = 2

    public func rewrites(over shown: NowPlayingShown?, forced: Bool, drift: Double) -> Bool {
        forced || self != shown || drift > Self.driftBeforeRewriteMs
    }

    public func commandsChanged(from shown: NowPlayingShown?) -> Bool {
        track.favourite != shown?.track.favourite || queueIndex != shown?.queueIndex
            || queueCount != shown?.queueCount || shuffle != shown?.shuffle
            || repeatMode != shown?.repeatMode || shown == nil
    }
}

public struct LandingJournal: Sendable {
    public static let key = "unfinished-sync-landing"
    public static let suite = "library-features"

    private let suiteName: String

    public init(suite: String = LandingJournal.suite) {
        suiteName = suite
    }

    private var settings: UserDefaults { UserDefaults(suiteName: suiteName) ?? .standard }

    public var left: String? { settings.string(forKey: Self.key) }

    public func begin(_ uri: String) {
        settings.set(uri, forKey: Self.key)
    }

    public func finish() {
        settings.removeObject(forKey: Self.key)
    }

    public func finish(ifStill uri: String) {
        guard left == uri else { return }
        finish()
    }
}
