import Foundation
import ResonateNative

extension EqualiserFilter {
    public static let every: [EqualiserFilter] = [
        .peaking, .lowShelf, .highShelf, .lowPass, .highPass, .notch, .bandPass, .allPass,
    ]

    public var label: String {
        switch self {
        case .peaking: "Peak"
        case .lowShelf: "Low shelf"
        case .highShelf: "High shelf"
        case .lowPass: "Low pass"
        case .highPass: "High pass"
        case .notch: "Notch"
        case .bandPass: "Band pass"
        case .allPass: "All pass"
        }
    }

    public var usesGain: Bool {
        switch self {
        case .peaking, .lowShelf, .highShelf: true
        case .lowPass, .highPass, .notch, .bandPass, .allPass: false
        }
    }
}

extension EqualiserCurve {
    public static let flat = EqualiserCurve(name: "", preampDb: 0, bands: [])

    public var summary: String {
        if !name.isEmpty { return name }
        return bands.isEmpty ? "Flat" : "Custom curve · \(counted(bands.count, "band"))"
    }

    public var presetName: String {
        let flat = preampDb == 0 && bands.allSatisfy { $0.filter.usesGain && $0.gainDb == 0 }
        return flat ? "Flat" : name
    }

    public func level(of band: EqualiserBand) -> Float {
        band.filter.usesGain ? band.gainDb + preampDb : preampDb
    }

    public func replacing(_ index: Int, with band: EqualiserBand) -> EqualiserCurve {
        var next = self
        if next.bands.indices.contains(index) { next.bands[index] = band }
        return next
    }
}

public enum EqualiserUnits {
    public static let semitone = Float(pow(2.0, 1.0 / 12.0))
    public static let qNotch = Float(pow(2.0, 1.0 / 6.0))
    public static let gainStep: Float = 0.5
    public static let addedQ: Float = 1.0

    public static func hertz(_ frequency: Float) -> String {
        frequency >= 1000
            ? String(format: frequency >= 10_000 ? "%.1f kHz" : "%.2f kHz", frequency / 1000)
            : String(format: frequency >= 100 ? "%.0f Hz" : "%.1f Hz", frequency)
    }

    public static func decibels(_ gain: Float) -> String {
        String(format: "%+.1f dB", gain)
    }

    public static func range(of hertz: Float) -> String {
        switch hertz {
        case ..<60: "Sub-bass"
        case ..<180: "Bass"
        case ..<400: "Warmth"
        case ..<900: "Low mids"
        case ..<1800: "Mids"
        case ..<3500: "Vocals"
        case ..<6000: "Presence"
        case ..<12000: "Treble"
        default: "Air"
        }
    }

    public static func q(_ q: Float) -> String {
        String(format: "Q %.2f", q)
    }

    public static func tidy(hertz: Float) -> Float {
        switch hertz {
        case ..<100: (hertz * 10).rounded() / 10
        case ..<1000: hertz.rounded()
        default: (hertz / 10).rounded() * 10
        }
    }

    public static func tidy(decibels: Float) -> Float {
        (decibels * 10).rounded() / 10
    }

    public static func tidy(q: Float) -> Float {
        (q * 100).rounded() / 100
    }
}

public struct EqualiserScale {
    public static let lowestHz: Float = 20
    public static let highestHz: Float = 20_000
    public static let narrowestSpan: Float = 15
    public static let widestSpan: Float = 30
    public static let markedEvery: Float = 5
    public static let labelledFrequencies: [(hertz: Float, label: String)] = [(100, "100"), (1000, "1k"), (10_000, "10k")]
    public static let fainterFrequencies: [Float] = [
        30, 40, 50, 60, 70, 80, 90, 200, 300, 400, 500, 600, 700, 800, 900,
        2000, 3000, 4000, 5000, 6000, 7000, 8000, 9000,
    ]

    public let size: CGSize
    public let span: Float

    public init(size: CGSize, span: Float) {
        self.size = size
        self.span = span
    }

    public static func span(around response: [Float], handles: [Float]) -> Float {
        let loudest = (response + handles).lazy.filter(\.isFinite).map(abs).max() ?? 0
        let rounded = (loudest / markedEvery).rounded(.up) * markedEvery
        return min(max(rounded, narrowestSpan), widestSpan)
    }

    public func x(_ hertz: Float) -> CGFloat {
        CGFloat(log10(hertz / Self.lowestHz) / log10(Self.highestHz / Self.lowestHz)) * size.width
    }

    public func hertz(atX x: CGFloat) -> Float {
        let fraction = Float(min(max(x / max(size.width, 1), 0), 1))
        return Self.lowestHz * pow(Self.highestHz / Self.lowestHz, fraction)
    }

    public func y(_ decibels: Float) -> CGFloat {
        let held = min(max(decibels, -span), span)
        return CGFloat((span - held) / (2 * span)) * size.height
    }

    public func decibels(atY y: CGFloat) -> Float {
        span - Float(y / max(size.height, 1)) * 2 * span
    }
}

