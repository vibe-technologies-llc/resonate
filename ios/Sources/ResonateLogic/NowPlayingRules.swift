import Foundation
import ResonateNative

public enum SessionInterruption: Equatable, Sendable {
    case began
    case ended(shouldResume: Bool)
}

public enum SessionAction: Equatable, Sendable {
    case nothing
    case pauseSounding
    case resume([UInt32])
}

public struct InterruptionPolicy: Sendable {
    private var interrupted = false
    private var owed: [UInt32] = []
    private var resuming = false

    public init() {}

    public mutating func heard(_ interruption: SessionInterruption?) -> SessionAction {
        switch interruption {
        case .began?:
            interrupted = true
            resuming = false
            return .pauseSounding
        case .ended?:
            interrupted = false
            return resumeOwed()
        case nil:
            return .nothing
        }
    }

    public mutating func paused(_ zones: [UInt32]) {
        guard interrupted else { return }
        owe(zones)
    }

    public mutating func cameForward() -> SessionAction {
        resumeOwed()
    }

    public mutating func servicesReset() -> SessionAction {
        resumeOwed()
    }

    public mutating func listenerSounded() -> SessionAction {
        interrupted = false
        return resumeOwed()
    }

    public mutating func refusedToSound(_ zones: [UInt32]) {
        owe(zones)
    }

    public mutating func resumeRefused() {
        resuming = false
    }

    public mutating func resumed() -> Bool {
        guard resuming else { return false }
        forget()
        return true
    }

    public mutating func listenerPaused() {
        forget()
    }

    public mutating func lostTheDevice() -> SessionAction {
        forget()
        return .pauseSounding
    }

    public var owedResume: [UInt32] { owed }

    private mutating func resumeOwed() -> SessionAction {
        guard !owed.isEmpty, !resuming else { return .nothing }
        resuming = true
        return .resume(owed)
    }

    private mutating func owe(_ zones: [UInt32]) {
        owed += zones.filter { !owed.contains($0) }
    }

    private mutating func forget() {
        interrupted = false
        owed = []
        resuming = false
    }
}

public struct SessionRelease: Sendable {
    private var lastTransport: Transport?
    private var lastZone: UInt32?
    private var wasSleeping = false
    private var elsewhereSounded = false

    public init() {}

    public mutating func owed(by snapshot: Snapshot, listening: Bool) -> Bool {
        let sleeping = snapshot.sleepRemainingMs != nil
        let elsewhereSounds = snapshot.heardElsewhere?.transport.sounds == true
        let sameZone = lastZone == snapshot.zone
        let stopped =
            sameZone && snapshot.transport == .stopped && lastTransport != nil && lastTransport != .stopped
        let sleptOff =
            sameZone && snapshot.transport == .paused && lastTransport == .playing && wasSleeping && !sleeping
        let lastHeardElsewhereFellSilent = elsewhereSounded && !elsewhereSounds && snapshot.transport == .stopped
        lastTransport = snapshot.transport
        lastZone = snapshot.zone
        wasSleeping = sleeping
        elsewhereSounded = elsewhereSounds
        return (stopped || sleptOff || lastHeardElsewhereFellSilent) && !elsewhereSounds && !listening
    }
}

public struct CommandAvailability: Equatable, Sendable {
    public let loaded: Bool
    public let steers: Bool
    public let next: Bool

    public init(_ shown: NowPlayingShown?) {
        loaded = shown != nil
        steers = loaded && shown?.zoneHeard == nil
        let lastRow = (shown?.queueIndex ?? 0) + 1 >= (shown?.queueCount ?? 0)
        let wraps = shown?.repeatMode == NowPlayingShown.repeatsAll
        next = steers && (!lastRow || wraps)
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
    public let zoneHeard: UInt32?

    public init(
        track: Track,
        transport: Transport,
        durationMs: UInt64,
        queueIndex: Int,
        queueCount: Int,
        shuffle: Bool,
        repeatMode: UInt32,
        artRevision: UInt64,
        artworkReady: Bool,
        zoneHeard: UInt32? = nil
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
        self.zoneHeard = zoneHeard
    }

    public static let driftBeforeRewriteMs: Double = 1500
    public static let repeatsAll: UInt32 = 2

    public var speaksFor: SessionSpeaksFor { zoneHeard == nil ? .shown : .elsewhere }

    public func rewrites(over shown: NowPlayingShown?, forced: Bool, drift: Double) -> Bool {
        forced || self != shown || drift > Self.driftBeforeRewriteMs
    }

    public func commandsChanged(from shown: NowPlayingShown?) -> Bool {
        track.favourite != shown?.track.favourite || queueIndex != shown?.queueIndex
            || queueCount != shown?.queueCount || shuffle != shown?.shuffle
            || repeatMode != shown?.repeatMode || zoneHeard != shown?.zoneHeard || shown == nil
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
