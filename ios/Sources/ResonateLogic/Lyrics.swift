import Foundation
import ResonateNative

#if canImport(CoreGraphics)
import CoreGraphics
#endif

public enum LyricPace {
    public static let lineSyncedTick: TimeInterval = 0.05
    public static let drawsBeyondPanes: CGFloat = 1
    public static let drawsWithin: CGFloat = 320
    public static let endOfLyrics = "END OF LYRICS"
    public static let endTracking: CGFloat = 1.4

    public static let turn: TimeInterval = 0.42
    public static let glide: TimeInterval = 0.6
    public static let launchedAtMost = 4.0
    public static let lagPerLine: TimeInterval = 0.014
    public static let lagsAtMost = 6
    public static let arrivesIn: TimeInterval = 0.56
    public static let rise: TimeInterval = 0.6
    public static let risePerLine: TimeInterval = 0.04
    public static let risesAtMost = 8
    public static let breath: TimeInterval = 2.4
    public static let driftsAtMost: TimeInterval = 0.25
    public static let steeredOver: TimeInterval = 0.55
    public static let handsOff: TimeInterval = 6
    public static let coastsFor: TimeInterval = 0.5
    public static let settled = 0.5
    public static let lit = 1.0
    public static let adrift = 0.8
    public static let unsungShare = 0.38
    public static let solidUnsungShare = 0.7
    public static let solidStandingFloor = 0.85
    public static let voiceOfTwoSpans = 0.84
    public static let dimDot = 0.22
    public static let dotSwellsBy = 0.23
    public static let breathDots = 3
    public static let bandedOver = 0.55
    public static let pageTurned = 0.8
    public static let looksQuietlyFor: Duration = .milliseconds(450)

    public static func share(_ elapsed: TimeInterval, of span: TimeInterval) -> Double {
        min(max(elapsed / span, 0), 1)
    }

    public static func easedInOut(_ through: Double) -> Double {
        if through < 0.5 { return 2 * through * through }
        let left = -2 * through + 2
        return 1 - left * left / 2
    }

    public static func landed(_ through: Double) -> Double {
        let left = 1 - through
        return 1 - (3 * through + 1) * left * left * left
    }

    public static func landingPace(_ through: Double) -> Double {
        12 * through * (1 - through) * (1 - through)
    }

    public static func launched(_ through: Double) -> Double {
        let left = 1 - through
        return through * left * left * left
    }

    public static func launchPace(_ through: Double) -> Double {
        let left = 1 - through
        return left * left * (1 - 4 * through)
    }

    public static func lagged(_ lines: Int, by perLine: TimeInterval, atMost: Int) -> TimeInterval {
        perLine * Double(min(max(lines, 0), atMost))
    }
}

public enum LyricFalloff: Equatable, Sendable {
    case around
    case across

    public func between(sung: Int, line: Int) -> Double {
        line < sung ? behind(sung - line) : ahead(line - sung)
    }

    public func ahead(_ away: Int) -> Double {
        switch (self, away) {
        case (_, 0): LyricPace.lit
        case (.around, 1): 0.4
        case (.around, 2): 0.18
        case (.around, _): 0
        case (.across, 1): 0.42
        case (.across, 2): 0.3
        case (.across, 3): 0.22
        case (.across, _): 0.16
        }
    }

    public func behind(_ away: Int) -> Double {
        switch self {
        case .around: 0
        case .across: ahead(away)
        }
    }

    public func spent(read: Int?, line: Int) -> Double {
        switch (self, read) {
        case (.around, let read?) where line >= read: ahead(line - read + 1)
        case (.around, _): 0
        case (.across, let read?): ahead(max(abs(read - line), 1))
        case (.across, nil): ahead(.max)
        }
    }
}

public enum LyricReading: Equatable, Sendable {
    case at(Int)
    case evenly
    case spent(Int?)

    public init(_ reads: LyricReads) {
        switch reads {
        case .at(let line): self = .at(Int(line))
        case .spent(let line): self = .spent(line.map(Int.init))
        case .evenly: self = .evenly
        }
    }
}

public struct LyricVoicesLit: Equatable, Sendable {
    public static let none = LyricVoicesLit(one: nil, two: nil)

    public let one: Int?
    public let two: Int?

    public init(one: Int?, two: Int?) {
        self.one = one
        self.two = two
    }

    public func lights(_ line: Int) -> Bool {
        one == line || two == line
    }
}

public struct LyricTurn<Value: Equatable>: Sendable where Value: Sendable {
    public private(set) var from: Value
    public private(set) var to: Value
    private var started: TimeInterval

    public init(still value: Value) {
        from = value
        to = value
        started = -.infinity
    }

    public mutating func onto(_ value: Value, at now: TimeInterval) {
        guard value != to else { return }
        from = to
        to = value
        started = now
    }

    public func blended(at now: TimeInterval, _ reading: (Value) -> Double) -> Double {
        let through = LyricPace.easedInOut(LyricPace.share(now - started, of: LyricPace.turn))
        return reading(from) * (1 - through) + reading(to) * through
    }

    public func settled(at now: TimeInterval) -> Bool {
        now - started >= LyricPace.turn
    }
}

public struct LyricLeg: Sendable {
    public let was: Double
    public let lands: Double
    public let launch: Double
    public let started: TimeInterval

    public init(was: Double, lands: Double, launch: Double, started: TimeInterval) {
        self.was = was
        self.lands = lands
        self.launch = launch
        self.started = started
    }

    public static func launched(from was: Double, to lands: Double, pace: Double, at now: TimeInterval) -> LyricLeg {
        let reach = abs(lands - was) * LyricPace.launchedAtMost
        return LyricLeg(was: was, lands: lands, launch: min(max(pace, -reach), reach), started: now)
    }

    public func elapsed(at now: TimeInterval) -> TimeInterval {
        max(now - started, 0)
    }

    public func after(_ elapsed: TimeInterval) -> Double {
        let through = LyricPace.share(elapsed, of: LyricPace.glide)
        return was + (lands - was) * LyricPace.landed(through) + launch * LyricPace.launched(through)
    }

    public func pace(after elapsed: TimeInterval) -> Double {
        let through = LyricPace.share(elapsed, of: LyricPace.glide)
        return (lands - was) * LyricPace.landingPace(through) + launch * LyricPace.launchPace(through)
    }

    public func landed(at now: TimeInterval) -> Bool {
        elapsed(at: now) >= LyricPace.glide
    }
}

public struct LyricGlide: Sendable {
    public let leg: LyricLeg
    public let before: LyricLeg?

    public init(leg: LyricLeg, before: LyricLeg?) {
        self.leg = leg
        self.before = before
    }

    public static func fromRest(was: Double, lands: Double, at now: TimeInterval) -> LyricGlide {
        LyricGlide(leg: LyricLeg(was: was, lands: lands, launch: 0, started: now), before: nil)
    }

    public func at(_ now: TimeInterval) -> Double {
        leg.after(leg.elapsed(at: now))
    }

    public func lagging(_ lag: TimeInterval, at now: TimeInterval) -> Double {
        let into = leg.elapsed(at: now)
        if let before, into < lag {
            return before.after(max(before.elapsed(at: now) - lag, 0))
        }
        return leg.after(max(into - lag, 0))
    }

    public func redirected(to lands: Double, at now: TimeInterval) -> LyricGlide {
        let into = leg.elapsed(at: now)
        let was = leg.after(into)
        return LyricGlide(leg: .launched(from: was, to: lands, pace: leg.pace(after: into), at: now), before: leg)
    }

    public func settled(at now: TimeInterval) -> Bool {
        leg.elapsed(at: now) >= LyricPace.glide + LyricPace.lagged(LyricPace.lagsAtMost, by: LyricPace.lagPerLine, atMost: LyricPace.lagsAtMost)
    }
}

public struct LyricHeard: Equatable, Sendable {
    public let track: UInt64
    public let playing: Bool

    public init(track: UInt64, playing: Bool) {
        self.track = track
        self.playing = playing
    }

    public static func of(_ snapshot: Snapshot?, showing track: Track?) -> LyricHeard? {
        guard let snapshot, let current = snapshot.current, let track, current.id == track.id else { return nil }
        return LyricHeard(track: current.id, playing: snapshot.transport == .playing)
    }
}

public struct LyricPlayback: Equatable, Sendable {
    public let track: UInt64
    public let positionMs: UInt64
    public let playing: Bool

    public init(track: UInt64, positionMs: UInt64, playing: Bool) {
        self.track = track
        self.positionMs = positionMs
        self.playing = playing
    }
}

public struct LyricClock: Sendable {
    private var heard: LyricPlayback?
    private var heardAt: TimeInterval = 0
    private var track: UInt64?
    private var from: TimeInterval = 0
    private var at: TimeInterval = 0
    private var running = false

    public init() {}

    public mutating func kept(_ playback: LyricPlayback, at now: TimeInterval) -> TimeInterval {
        if heard != playback {
            heard = playback
            heardAt = now
        }
        let sounding = playback.playing ? max(now - heardAt, 0) : 0
        let sampled = Double(playback.positionMs) / 1000 + sounding
        let elapsed = max(now - at, 0)
        let ran = from + elapsed

        guard running, track == playback.track, playback.playing, abs(ran - sampled) <= LyricPace.driftsAtMost else {
            track = playback.track
            from = sampled
            at = now
            running = playback.playing
            return sampled
        }

        let share = 1 - exp(-elapsed / LyricPace.steeredOver)
        let steered = max(ran + (sampled - ran) * share, from)
        from = steered
        at = now
        return steered
    }

    public mutating func stop() {
        self = LyricClock()
    }
}

public struct LyricAsked: Equatable, Sendable {
    public let track: UInt64
    public let positionMs: UInt64

    public init(track: UInt64, positionMs: UInt64) {
        self.track = track
        self.positionMs = positionMs
    }
}

public struct LyricBreathing: Sendable {
    public let line: Int
    public let started: TimeInterval

    public init(line: Int, started: TimeInterval) {
        self.line = line
        self.started = started
    }

    public func opacity(at now: TimeInterval) -> Double {
        LyricPace.easedInOut(LyricPace.share(now - started, of: LyricPace.turn))
    }

    public func fading(at now: TimeInterval) -> Double {
        1 - LyricPace.share(now - started, of: LyricPace.turn)
    }

    public func settled(at now: TimeInterval) -> Bool {
        now - started >= LyricPace.turn
    }
}

public struct LyricBreath: Sendable {
    public let through: Double
    public let opacity: Double

    public init(through: Double, opacity: Double) {
        self.through = through
        self.opacity = opacity
    }
}

public struct LyricHand: Sendable {
    public var at: TimeInterval?
    public var grabbedFrom: Double?
    public var passedOver = false
    public var coast: LyricLeg?

    public init(at: TimeInterval? = nil, grabbedFrom: Double? = nil, passedOver: Bool = false, coast: LyricLeg? = nil) {
        self.at = at
        self.grabbedFrom = grabbedFrom
        self.passedOver = passedOver
        self.coast = coast
    }
}

public struct LyricLandmarks: Equatable, Sendable {
    public let readAt: UInt32?
    public let reads: LyricReads
    public let waitingFor: UInt32?
    public let ended: Bool
    public let voiceOne: UInt32?
    public let voiceTwo: UInt32?

    public init(_ frame: LyricFrame) {
        readAt = frame.readAt
        reads = frame.reads
        waitingFor = frame.waiting?.next
        ended = frame.ended
        voiceOne = frame.voiceOne
        voiceTwo = frame.voiceTwo
    }
}
