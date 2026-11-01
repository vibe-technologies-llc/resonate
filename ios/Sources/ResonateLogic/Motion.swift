import Foundation

public struct MotionCurve: Equatable, Sendable {
    public let x1: Double
    public let y1: Double
    public let x2: Double
    public let y2: Double

    public init(_ x1: Double, _ y1: Double, _ x2: Double, _ y2: Double) {
        self.x1 = x1
        self.y1 = y1
        self.x2 = x2
        self.y2 = y2
    }

    public static let settling = MotionCurve(1.0 / 3, 1, 2.0 / 3, 1)
    public static let smooth = MotionCurve(1.0 / 3, 0, 2.0 / 3, 1)

    private static let solverRounds = 40

    public func progress(at time: Double) -> Double {
        let clamped = min(max(time, 0), 1)
        var low = 0.0
        var high = 1.0
        for _ in 0..<Self.solverRounds {
            let middle = (low + high) / 2
            if Self.bezier(middle, x1, x2) < clamped { low = middle } else { high = middle }
        }
        return Self.bezier((low + high) / 2, y1, y2)
    }

    private static func bezier(_ along: Double, _ first: Double, _ second: Double) -> Double {
        let rest = 1 - along
        return 3 * rest * rest * along * first + 3 * rest * along * along * second + along * along * along
    }
}

public enum MotionSpan {
    public static let arrivesOver = 0.15
    public static let flipsOver = 0.14
    public static let handsOver = 0.28
}

public enum MotionKind: Equatable, Sendable {
    case handover
    case arrival
    case flip
    case progress
}

public enum MotionRendering: Equatable, Sendable {
    case animated
    case fades
    case instant

    public static func of(_ kind: MotionKind, reduceMotion: Bool, sceneActive: Bool) -> MotionRendering {
        guard sceneActive else { return .instant }
        guard reduceMotion else { return .animated }
        switch kind {
        case .arrival: return .fades
        case .handover, .flip, .progress: return .instant
        }
    }
}

public enum SongChange: Equatable, Sendable {
    case unchanged
    case arrived
    case handed
    case cleared

    public init(from shown: String?, to now: String?) {
        switch (shown, now) {
        case (nil, nil): self = .unchanged
        case (nil, _?): self = .arrived
        case (_?, nil): self = .cleared
        case (let before?, let after?): self = before == after ? .unchanged : .handed
        }
    }

    public var handsOver: Bool { self != .unchanged }
}

public enum ProgressStep: Equatable, Sendable {
    case snap(to: Double)
    case flow(to: Double, over: Double)
}

public enum ProgressFlow {
    public static let flowsWithin: ClosedRange<Double> = 0.05...0.6
    public static let flowsAcrossMs = 1500.0
    private static let millisecondsPerSecond = 1000.0

    public struct Reading: Equatable, Sendable {
        public let old: Double
        public let now: Double
        public let sinceLast: Double?

        public init(old: Double, now: Double, sinceLast: Double?) {
            self.old = old
            self.now = now
            self.sinceLast = sinceLast
        }
    }

    public struct Conditions: Equatable, Sendable {
        public let playing: Bool
        public let touched: Bool
        public let held: Double?
        public let reduceMotion: Bool
        public let songChanged: Bool

        public init(playing: Bool, touched: Bool, held: Double?, reduceMotion: Bool, songChanged: Bool) {
            self.playing = playing
            self.touched = touched
            self.held = held
            self.reduceMotion = reduceMotion
            self.songChanged = songChanged
        }
    }

    public static func step(_ reading: Reading, _ conditions: Conditions) -> ProgressStep {
        if let held = conditions.held { return .snap(to: held) }
        let stepped = reading.now - reading.old
        let elapsed = min(max(reading.sinceLast ?? flowsWithin.lowerBound, flowsWithin.lowerBound), flowsWithin.upperBound)
        let flows =
            reading.sinceLast != nil && !conditions.touched && !conditions.songChanged
            && conditions.playing && stepped >= 0 && stepped < flowsAcrossMs
            && MotionRendering.of(.progress, reduceMotion: conditions.reduceMotion, sceneActive: true) == .animated
        return flows ? .flow(to: reading.now + elapsed * millisecondsPerSecond, over: elapsed) : .snap(to: reading.now)
    }
}

public struct HeldHeight: Equatable, Sendable {
    private static let sameWidthWithin = 0.5

    public private(set) var width = 0.0
    public private(set) var height = 0.0

    public init() {}

    public mutating func see(width seen: Double, height tall: Double) {
        if abs(seen - width) > Self.sameWidthWithin {
            width = seen
            height = tall
        } else if tall > height {
            height = tall
        }
    }
}

public enum SliderDrag: Equatable, Sendable {
    case undecided
    case sliding
    case declined

    public static let decidedPast = 8.0

    public func after(across: Double, down: Double) -> SliderDrag {
        guard self == .undecided, max(abs(across), abs(down)) >= Self.decidedPast else { return self }
        return abs(down) > abs(across) ? .declined : .sliding
    }
}
