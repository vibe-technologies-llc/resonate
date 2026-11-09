import Foundation
import ResonateNative

#if canImport(CoreGraphics)
import CoreGraphics
#endif

public enum EqualiserShown {
    public static func curve(_ committed: EqualiserCurve, octaves: EqualiserCurve) -> EqualiserCurve {
        guard committed.bands.isEmpty else { return committed }
        var shown = octaves
        shown.name = committed.name
        shown.preampDb = committed.preampDb
        return shown
    }
}

public enum BandSelection {
    public static func shown(_ chosen: Int?, among count: Int) -> Int? {
        guard count > 0 else { return nil }
        guard let chosen, chosen >= 0, chosen < count else { return 0 }
        return chosen
    }

    public static func afterRemoving(_ removed: Int, leaving count: Int) -> Int? {
        guard count > 0 else { return nil }
        return min(max(removed, 0), count - 1)
    }
}

public enum BandPlacement {
    public static let aloneAtHz: Float = 1_000
    static let sameWidthWithin: Float = 0.01

    public static func added(to curve: EqualiserCurve, limits: EqualiserLimits) -> EqualiserBand? {
        guard curve.bands.count < Int(limits.bandsAtMost) else { return nil }
        let hertz = freeHertz(among: curve.bands.map(\.frequencyHz), lowest: limits.lowestHz, highest: limits.highestHz)
        return EqualiserBand(filter: .peaking, frequencyHz: hertz, gainDb: 0, q: EqualiserUnits.addedQ)
    }

    public static func freeHertz(among taken: [Float], lowest: Float, highest: Float) -> Float {
        let inside = taken.filter { $0.isFinite && $0 > lowest && $0 < highest }.sorted()
        guard !inside.isEmpty else { return min(max(aloneAtHz, lowest), highest) }
        let edges = [lowest] + inside + [highest]
        var widest: (octaves: Float, middle: Float)?
        for (low, high) in zip(edges, edges.dropFirst()) {
            let octaves = log2(high / low)
            let middle = (low * high).squareRoot()
            guard let best = widest else {
                widest = (octaves, middle)
                continue
            }
            if octaves > best.octaves + sameWidthWithin
                || (abs(octaves - best.octaves) <= sameWidthWithin && nearer(middle, than: best.middle))
            {
                widest = (octaves, middle)
            }
        }
        let middle = widest?.middle ?? aloneAtHz
        return min(max(EqualiserUnits.tidy(hertz: middle), lowest), highest)
    }

    static func nearer(_ hertz: Float, than other: Float) -> Bool {
        abs(log2(hertz / aloneAtHz)) < abs(log2(other / aloneAtHz)) - sameWidthWithin
    }
}

public struct EqualiserHandle: Equatable, Sendable {
    public let band: Int
    public let centre: CGPoint
    public let reach: CGRect
    public let layer: Int
}

public enum EqualiserHandles {
    public static let reachAtLeast: CGFloat = 44
    public static let drawnEdge: CGFloat = 28

    public static func laid(_ curve: EqualiserCurve, on scale: EqualiserScale, selected: Int?) -> [EqualiserHandle] {
        let half = reachAtLeast / 2
        return curve.bands.indices.map { index in
            let band = curve.bands[index]
            let centre = CGPoint(x: scale.x(band.frequencyHz), y: scale.y(curve.level(of: band)))
            return EqualiserHandle(
                band: index,
                centre: centre,
                reach: CGRect(x: centre.x - half, y: centre.y - half, width: reachAtLeast, height: reachAtLeast),
                layer: index == selected ? curve.bands.count : index
            )
        }
    }

    public static func hit(_ point: CGPoint, among handles: [EqualiserHandle]) -> Int? {
        handles.filter { $0.reach.contains(point) }.max { $0.layer < $1.layer }?.band
    }
}

public enum EqualiserDrag {
    public static let tapTravel: CGFloat = 4

    public static func travelled(_ translation: CGSize) -> Bool {
        (translation.width * translation.width + translation.height * translation.height).squareRoot() > tapTravel
    }

    public static func moved(
        _ band: EqualiserBand,
        from start: CGPoint,
        by translation: CGSize,
        on scale: EqualiserScale,
        preamp: Float,
        limits: EqualiserLimits
    ) -> EqualiserBand {
        let x = start.x + translation.width
        let y = start.y + translation.height
        var next = band
        next.frequencyHz = min(max(EqualiserUnits.tidy(hertz: scale.hertz(atX: x)), limits.lowestHz), limits.highestHz)
        if band.filter.usesGain {
            let gain = EqualiserUnits.tidy(decibels: scale.decibels(atY: y) - preamp)
            next.gainDb = min(max(gain, -limits.gainAtMostDb), limits.gainAtMostDb)
        }
        return next
    }
}

public enum BandMeasure: String, CaseIterable, Sendable {
    case gain
    case frequency
    case width
    case preamp

    public var title: String {
        switch self {
        case .gain: "Gain"
        case .frequency: "Frequency"
        case .width: "Width"
        case .preamp: "Level"
        }
    }
}

public struct BandAxis: Equatable, Sendable {
    public static let shownAtLeastDb: Float = 12
    public static let shownInStepsOfDb: Float = 6

    public let measure: BandMeasure
    public let lowest: Float
    public let highest: Float
    public let least: Float
    public let most: Float

    public init(measure: BandMeasure, lowest: Float, highest: Float, least: Float, most: Float) {
        self.measure = measure
        self.lowest = lowest
        self.highest = highest
        self.least = least
        self.most = most
    }

    public static func gain(around value: Float, limits: EqualiserLimits) -> BandAxis {
        let reach = reach(around: value, atMost: limits.gainAtMostDb)
        return BandAxis(measure: .gain, lowest: -reach, highest: reach, least: -limits.gainAtMostDb, most: limits.gainAtMostDb)
    }

    public static func preamp(around value: Float, limits: EqualiserLimits) -> BandAxis {
        let reach = reach(around: value, atMost: limits.preampAtMostDb)
        return BandAxis(measure: .preamp, lowest: -reach, highest: reach, least: -limits.preampAtMostDb, most: limits.preampAtMostDb)
    }

    public static func frequency(_ limits: EqualiserLimits) -> BandAxis {
        BandAxis(measure: .frequency, lowest: limits.lowestHz, highest: limits.highestHz, least: limits.lowestHz, most: limits.highestHz)
    }

    public static func width(_ limits: EqualiserLimits) -> BandAxis {
        BandAxis(measure: .width, lowest: limits.qAtLeast, highest: limits.qAtMost, least: limits.qAtLeast, most: limits.qAtMost)
    }

    static func reach(around value: Float, atMost: Float) -> Float {
        let needed = value.isFinite ? (abs(value) / shownInStepsOfDb).rounded(.up) * shownInStepsOfDb : 0
        return min(atMost, max(shownAtLeastDb, needed))
    }

    var logarithmic: Bool {
        measure == .frequency || measure == .width
    }

    public func position(of value: Float) -> Double {
        guard highest > lowest, value.isFinite else { return 0 }
        let held = min(max(value, lowest), highest)
        if logarithmic { return Double(log(held / lowest) / log(highest / lowest)) }
        return Double((held - lowest) / (highest - lowest))
    }

    public func value(at position: Double) -> Float {
        let fraction = Float(min(max(position.isFinite ? position : 0, 0), 1))
        let raw = logarithmic ? lowest * pow(highest / lowest, fraction) : lowest + (highest - lowest) * fraction
        return min(max(tidied(raw), lowest), highest)
    }

    public func stepped(_ value: Float, up: Bool) -> Float {
        let next: Float =
            switch measure {
            case .gain, .preamp: value + (up ? EqualiserUnits.gainStep : -EqualiserUnits.gainStep)
            case .frequency: up ? value * EqualiserUnits.semitone : value / EqualiserUnits.semitone
            case .width: up ? value * EqualiserUnits.qNotch : value / EqualiserUnits.qNotch
            }
        return allowed(tidied(next))
    }

    public func written(_ value: Float) -> String {
        switch measure {
        case .gain, .preamp: EqualiserUnits.decibels(value)
        case .frequency: EqualiserUnits.hertz(value)
        case .width: EqualiserUnits.q(value)
        }
    }

    public func spoken(_ value: Float) -> String {
        switch measure {
        case .gain, .preamp: String(format: "%+.1f decibels", Double(value))
        case .frequency:
            value >= 1_000
                ? String(format: "%.2f kilohertz", Double(value / 1_000))
                : String(format: value >= 100 ? "%.0f hertz" : "%.1f hertz", Double(value))
        case .width: String(format: "Q %.2f", Double(value))
        }
    }

    public var range: String {
        "From \(written(least)) to \(written(most))"
    }

    public func entering(band number: Int?) -> String {
        guard let number else { return "Preamp level" }
        return "Band \(number) \(measure.title.lowercased())"
    }

    public func typing(_ value: Float) -> String {
        String(format: "%g", Double(tidied(value)))
    }

    public func read(_ typed: String) -> Float? {
        var text = typed.lowercased().filter { !$0.isWhitespace }.replacingOccurrences(of: "\u{2212}", with: "-")
        var factor: Float = 1
        switch measure {
        case .gain, .preamp:
            if text.hasSuffix("db") { text.removeLast(2) }
        case .frequency:
            if text.hasSuffix("khz") {
                text.removeLast(3)
                factor = 1_000
            } else if text.hasSuffix("hz") {
                text.removeLast(2)
            } else if text.hasSuffix("k") {
                text.removeLast()
                factor = 1_000
            }
        case .width:
            if text.hasPrefix("q") { text.removeFirst() }
        }
        text = measure == .frequency && Self.groupsThousands(text)
            ? text.replacingOccurrences(of: ",", with: "")
            : text.replacingOccurrences(of: ",", with: ".")
        guard let number = Float(text), number.isFinite else { return nil }
        return allowed(tidied(number * factor))
    }

    static func groupsThousands(_ text: String) -> Bool {
        let parts = text.split(separator: ",", omittingEmptySubsequences: false)
        guard parts.count > 1, let first = parts.first, (1...3).contains(first.count) else { return false }
        return parts.allSatisfy { $0.allSatisfy(\.isNumber) } && parts.dropFirst().allSatisfy { $0.count == 3 }
    }

    func tidied(_ value: Float) -> Float {
        switch measure {
        case .gain, .preamp: EqualiserUnits.tidy(decibels: value)
        case .frequency: EqualiserUnits.tidy(hertz: value)
        case .width: EqualiserUnits.tidy(q: value)
        }
    }

    func allowed(_ value: Float) -> Float {
        min(max(value, least), most)
    }
}

public enum BandNudge {
    public static func level(_ band: EqualiserBand, up: Bool, limits: EqualiserLimits) -> EqualiserBand {
        var next = band
        if band.filter.usesGain {
            next.gainDb = BandAxis.gain(around: band.gainDb, limits: limits).stepped(band.gainDb, up: up)
        } else {
            next.q = BandAxis.width(limits).stepped(band.q, up: up)
        }
        return next
    }

    public static func frequency(_ band: EqualiserBand, up: Bool, limits: EqualiserLimits) -> EqualiserBand {
        var next = band
        let moved = up ? band.frequencyHz * EqualiserUnits.thirdOctave : band.frequencyHz / EqualiserUnits.thirdOctave
        next.frequencyHz = min(max(EqualiserUnits.tidy(hertz: moved), limits.lowestHz), limits.highestHz)
        return next
    }
}

public struct EqualiserDraft: Equatable, Sendable {
    public private(set) var curve: EqualiserCurve?
    public private(set) var holding = false

    public init() {}

    public func shown(over committed: EqualiserCurve) -> EqualiserCurve {
        curve ?? committed
    }

    public mutating func hold(_ moving: EqualiserCurve) {
        curve = moving
        holding = true
    }

    public mutating func release(_ kept: EqualiserCurve) {
        curve = kept
        holding = false
    }

    public mutating func drop() {
        curve = nil
        holding = false
    }

    public mutating func arrived(_ committed: EqualiserCurve?) {
        if curve == committed || !holding { curve = nil }
    }

    public mutating func settle(_ kept: EqualiserCurve) {
        if !holding, curve == kept { curve = nil }
    }
}

public struct TellingPace: Sendable {
    public static let every: Duration = .milliseconds(50)

    private let every: Duration
    private var toldAt: ContinuousClock.Instant?

    public init(every: Duration = TellingPace.every) {
        self.every = every
    }

    public mutating func ask(at now: ContinuousClock.Instant) -> Duration? {
        if let toldAt, now - toldAt < every { return every - (now - toldAt) }
        toldAt = now
        return nil
    }

    public mutating func told(at now: ContinuousClock.Instant) {
        toldAt = now
    }

    public mutating func forget() {
        toldAt = nil
    }
}

public enum HeadphoneMatches {
    public static let shownAtMost = 20

    public static func shown<Found>(_ found: [Found]) -> (shown: ArraySlice<Found>, more: Int) {
        (found.prefix(shownAtMost), max(found.count - shownAtMost, 0))
    }
}
