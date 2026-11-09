import Foundation
import ResonateNative

#if canImport(CoreGraphics)
import CoreGraphics
#endif

extension EqualiserFilter {
    public static let every: [EqualiserFilter] = [
        .peaking, .lowShelf, .highShelf, .lowPass, .highPass, .notch, .bandPass, .allPass,
    ]

    public var label: String {
        switch self {
        case .peaking: said("equaliser.filter.peak")
        case .lowShelf: said("equaliser.filter.low-shelf")
        case .highShelf: said("equaliser.filter.high-shelf")
        case .lowPass: said("equaliser.filter.low-pass")
        case .highPass: said("equaliser.filter.high-pass")
        case .notch: said("equaliser.filter.notch")
        case .bandPass: said("equaliser.filter.band-pass")
        case .allPass: said("equaliser.filter.all-pass")
        }
    }

    public var gainUnused: String {
        switch self {
        case .peaking: said("equaliser.filter.peak.gain-unused")
        case .lowShelf: said("equaliser.filter.low-shelf.gain-unused")
        case .highShelf: said("equaliser.filter.high-shelf.gain-unused")
        case .lowPass: said("equaliser.filter.low-pass.gain-unused")
        case .highPass: said("equaliser.filter.high-pass.gain-unused")
        case .notch: said("equaliser.filter.notch.gain-unused")
        case .bandPass: said("equaliser.filter.band-pass.gain-unused")
        case .allPass: said("equaliser.filter.all-pass.gain-unused")
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
        return bands.isEmpty ? said("equaliser.curve.flat") : said("equaliser.curve.custom", bands.count)
    }

    public var presetName: String {
        let flat = preampDb == 0 && bands.allSatisfy { $0.filter.usesGain && $0.gainDb == 0 }
        return flat ? said("equaliser.curve.flat") : name
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
    public static let thirdOctave = Float(pow(2.0, 1.0 / 3.0))
    public static let gainStep: Float = 0.5
    public static let addedQ: Float = 1.0

    public static func hertz(_ frequency: Float) -> String {
        frequency >= 1000
            ? said("units.khz", String(format: frequency >= 10_000 ? "%.1f" : "%.2f", frequency / 1000))
            : said("units.hz", String(format: frequency >= 100 ? "%.0f" : "%.1f", frequency))
    }

    public static func decibels(_ gain: Float) -> String {
        said("units.db", String(format: "%+.1f", gain))
    }

    public static func range(of hertz: Float) -> String {
        switch hertz {
        case ..<60: said("equaliser.range.sub-bass")
        case ..<180: said("equaliser.range.bass")
        case ..<400: said("equaliser.range.warmth")
        case ..<900: said("equaliser.range.low-mids")
        case ..<1800: said("equaliser.range.mids")
        case ..<3500: said("equaliser.range.vocals")
        case ..<6000: said("equaliser.range.presence")
        case ..<12000: said("equaliser.range.treble")
        default: said("equaliser.range.air")
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

public struct EqualiserScale: Equatable, Sendable {
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

