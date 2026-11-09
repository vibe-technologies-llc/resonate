import Foundation

#if canImport(CoreGraphics)
import CoreGraphics
#endif

public struct HeardBands: Equatable, Sendable {
    public static let count = 4
    public static let silentDecibels: Float = -120
    public static let silence = HeardBands(decibels: Array(repeating: silentDecibels, count: count))

    private static let windowEdgesWereSetFor = 256
    private static let binEdges = [1, 3, 7, 21, 91]

    public let decibels: [Float]

    init(decibels: [Float]) {
        self.decibels = decibels
    }

    public init(heard samples: [Float]) {
        guard samples.count >= Self.windowEdgesWereSetFor / 8 else {
            self = .silence
            return
        }

        let windowed = Self.hann(samples)
        let fullScale = Self.fullScaleMagnitude(of: samples.count)
        let edges = Self.edges(for: samples.count)

        decibels = zip(edges, edges.dropFirst()).map { low, high in
            let bins = low..<high
            let power = bins.reduce(Float(0)) { $0 + Self.power(of: windowed, at: $1) } / Float(bins.count)
            return Self.decibels(power / (fullScale * fullScale))
        }
    }

    private static func edges(for length: Int) -> [Int] {
        let highest = length / 2
        let scale = Float(length) / Float(windowEdgesWereSetFor)
        var edges: [Int] = []
        for edge in binEdges {
            let scaled = min(max(Int((Float(edge) * scale).rounded()), 1), highest)
            edges.append(max(scaled, (edges.last ?? 0) + 1))
        }
        return edges
    }

    private static func hann(_ samples: [Float]) -> [Float] {
        let span = Float(samples.count - 1)
        return samples.enumerated().map { index, sample in
            sample * (0.5 - 0.5 * cos(2 * .pi * Float(index) / span))
        }
    }

    private static func fullScaleMagnitude(of length: Int) -> Float {
        Float(length - 1) / 4
    }

    private static func power(of samples: [Float], at bin: Int) -> Float {
        let coefficient = 2 * cos(2 * .pi * Float(bin) / Float(samples.count))
        var previous: Float = 0
        var earlier: Float = 0
        for sample in samples {
            let current = sample + coefficient * previous - earlier
            earlier = previous
            previous = current
        }
        return max(previous * previous + earlier * earlier - coefficient * previous * earlier, 0)
    }

    private static func decibels(_ power: Float) -> Float {
        guard power > 0 else { return silentDecibels }
        return max(10 * log10(power), silentDecibels)
    }
}

public struct PlayingBars: Equatable, Sendable {
    public static let resting: Float = 0.14
    public static let still: [Float] = [0.5, 0.85, 0.35, 0.65]

    static let spannedDecibels: Float = 24
    static let referenceFallsEachHearing: Float = 0.15
    static let quietestReference: Float = -60
    static let risingShare: Float = 0.7
    static let fallingShare: Float = 0.35

    public private(set) var heights: [Float] = still
    private var references = Array(repeating: quietestReference, count: HeardBands.count)

    public init() {}

    public mutating func hear(_ bands: HeardBands) {
        references = zip(bands.decibels, references).map { heard, reference in
            max(heard, reference - Self.referenceFallsEachHearing, Self.quietestReference)
        }

        let targets = zip(bands.decibels, references).map { heard, reference in
            let reached = (heard - reference + Self.spannedDecibels) / Self.spannedDecibels
            return Self.resting + (1 - Self.resting) * min(max(reached, 0), 1)
        }
        heights = zip(heights, targets).map { shown, target in
            shown + (target - shown) * (target > shown ? Self.risingShare : Self.fallingShare)
        }
    }

    public mutating func rest() {
        heights = Self.still
    }
}

public struct PlayingBarsShape: Equatable, Sendable {
    public static let barWidthShare: CGFloat = 0.2
    public static let gapShare: CGFloat = 0.14

    public let height: CGFloat

    public init(height: CGFloat) {
        self.height = height
    }

    public var barWidth: CGFloat {
        height * Self.barWidthShare
    }

    public var gap: CGFloat {
        height * Self.gapShare
    }

    public var width: CGFloat {
        CGFloat(HeardBands.count) * barWidth + CGFloat(HeardBands.count - 1) * gap
    }

    public func middle(of bar: Int) -> CGFloat {
        CGFloat(bar) * (barWidth + gap) + barWidth / 2
    }

    public func barHeight(_ share: Float) -> CGFloat {
        max(height * CGFloat(share), barWidth)
    }
}
