import Foundation

public enum Oklab {
    public static func lab(fromLinear rgb: SIMD3<Float>) -> SIMD3<Float> {
        let lms = SIMD3<Float>(
            0.4122214708 * rgb.x + 0.5363325363 * rgb.y + 0.0514459929 * rgb.z,
            0.2119034982 * rgb.x + 0.6806995451 * rgb.y + 0.1073969566 * rgb.z,
            0.0883024619 * rgb.x + 0.2817188376 * rgb.y + 0.6299787005 * rgb.z
        )
        let root = SIMD3<Float>(cubeRoot(lms.x), cubeRoot(lms.y), cubeRoot(lms.z))
        return SIMD3(
            0.2104542553 * root.x + 0.7936177850 * root.y - 0.0040720468 * root.z,
            1.9779984951 * root.x - 2.4285922050 * root.y + 0.4505937099 * root.z,
            0.0259040371 * root.x + 0.7827717662 * root.y - 0.8086757660 * root.z
        )
    }

    public static func linear(fromLab lab: SIMD3<Float>) -> SIMD3<Float> {
        let root = SIMD3<Float>(
            lab.x + 0.3963377774 * lab.y + 0.2158037573 * lab.z,
            lab.x - 0.1055613458 * lab.y - 0.0638541728 * lab.z,
            lab.x - 0.0894841775 * lab.y - 1.2914855480 * lab.z
        )
        let lms = root * root * root
        return SIMD3(
            4.0767416621 * lms.x - 3.3077115913 * lms.y + 0.2309699292 * lms.z,
            -1.2684380046 * lms.x + 2.6097574011 * lms.y - 0.3413193965 * lms.z,
            -0.0041960863 * lms.x - 0.7034186147 * lms.y + 1.7076147010 * lms.z
        )
    }

    public static func polar(lightness: Float, chroma: Float, hue: Float) -> SIMD3<Float> {
        SIMD3(lightness, chroma * Float(cos(Double(hue))), chroma * Float(sin(Double(hue))))
    }

    public static func chroma(of lab: SIMD3<Float>) -> Float {
        (lab.y * lab.y + lab.z * lab.z).squareRoot()
    }

    private static func cubeRoot(_ value: Float) -> Float {
        Float(cbrt(Double(value)))
    }
}

public struct Palette: Sendable, Equatable {
    public static let size = 5
    private static let accentWeights: [Float] = [0.3, 0.2, 0.2, 0.15, 0.15]
    private static let accentVariations: [(hue: Float, lightness: Float, chroma: Float)] = [
        (0, 0, 1),
        (-0.35, -0.08, 0.85),
        (0.3, 0.05, 0.9),
        (-0.15, 0.1, 0.6),
        (0.5, -0.12, 0.75),
    ]
    public static let darkest: Float = 0.3
    public static let darkestGrey: Float = 0.16
    private static let lightest: Float = 0.64
    private static let hueless: Float = 0.015
    private static let dullest: Float = 0.07
    private static let richest: Float = 0.19
    private static let chromaLift: Float = 1.2
    private static let gamutSteps = 12
    private static let gamutShrink: Float = 0.88

    public let colours: [SIMD3<Float>]
    public let weights: [Float]

    private init(colours: [SIMD3<Float>], weights: [Float]) {
        self.colours = colours
        self.weights = weights
    }

    public static func around(accent: SIMD3<Float>) -> Palette {
        let lab = Oklab.lab(fromLinear: accent)
        let chroma = (lab.y * lab.y + lab.z * lab.z).squareRoot()
        let hue = Float(atan2(Double(lab.z), Double(lab.y)))
        let colours = accentVariations.map { variation in
            richened(Oklab.polar(
                lightness: lab.x + variation.lightness,
                chroma: chroma * variation.chroma,
                hue: hue + variation.hue
            ))
        }
        return Palette(colours: colours, weights: accentWeights)
    }

    public static func shaped(
        _ clusters: [(centre: SIMD3<Float>, members: Int)],
        smallestWeight: Float,
        darkest floor: Float = darkest
    ) -> Palette {
        let ranked = clusters.sorted { $0.members > $1.members }
        let total = Float(max(ranked.reduce(0) { $0 + $1.members }, 1))
        let raised = ranked.map { max(Float($0.members) / total, smallestWeight) }
        let sum = raised.reduce(0, +)

        return Palette(colours: ranked.map { richened($0.centre, darkest: floor) }, weights: raised.map { $0 / sum })
    }

    public func blended(toward other: Palette, by amount: Float) -> Palette {
        Palette(
            colours: zip(colours, other.colours).map { $0 + ($1 - $0) * amount },
            weights: zip(weights, other.weights).map { $0 + ($1 - $0) * amount }
        )
    }

    private static func richened(_ lab: SIMD3<Float>, darkest floor: Float = darkest) -> SIMD3<Float> {
        let chroma = (lab.y * lab.y + lab.z * lab.z).squareRoot()
        let hue = Float(atan2(Double(lab.z), Double(lab.y)))
        let lightness = min(max(lab.x, floor), lightest)
        var fitted = chroma < hueless ? chroma : min(max(chroma * chromaLift, dullest), richest)

        for _ in 0..<gamutSteps {
            let linear = Oklab.linear(fromLab: Oklab.polar(lightness: lightness, chroma: fitted, hue: hue))
            if linear.min() >= 0, linear.max() <= 1 { return linear }
            fitted *= gamutShrink
        }
        let linear = Oklab.linear(fromLab: Oklab.polar(lightness: lightness, chroma: fitted, hue: hue))
        return linear.clamped(lowerBound: SIMD3(repeating: 0), upperBound: SIMD3(repeating: 1))
    }
}

public struct BackdropTone: Equatable, Sendable {
    public static let dark = BackdropTone(lightnessFloor: 0.06, lightnessRange: 0.74, chroma: 1, vignette: 0.32)
    public static let light = BackdropTone(lightnessFloor: 0.52, lightnessRange: 0.4, chroma: 0.7, vignette: 0.12)

    public let lightnessFloor: Float
    public let lightnessRange: Float
    public let chroma: Float
    public let vignette: Float

    public init(lightnessFloor: Float, lightnessRange: Float, chroma: Float, vignette: Float) {
        self.lightnessFloor = lightnessFloor
        self.lightnessRange = lightnessRange
        self.chroma = chroma
        self.vignette = vignette
    }

    public static let neutralIntensity: Float = 0.5
    private static let brightestTop: Float = 0.95

    public func at(intensity: Float, darkening: Float) -> BackdropTone {
        let lean = (min(max(intensity, 0), 1) - Self.neutralIntensity) * 2
        guard lean != 0 else { return self }
        let darker = max(-lean, 0)
        let livelier = max(lean, 0)
        let dimming = 1 - darkening * darker
        let top = min((lightnessFloor + lightnessRange) * dimming * (1 + 0.12 * livelier), max(Self.brightestTop, lightnessFloor + lightnessRange))
        let floor = lightnessFloor * dimming

        return BackdropTone(
            lightnessFloor: floor,
            lightnessRange: top - floor,
            chroma: chroma * (1 - 0.4 * darker + 0.5 * livelier),
            vignette: max(vignette + 0.15 * darker - 0.08 * livelier, 0)
        )
    }

    public static func nowPlaying(dark isDark: Bool, intensity: Float) -> BackdropTone {
        isDark ? Self.dark.at(intensity: intensity, darkening: 0.5) : Self.light.at(intensity: intensity, darkening: 0.25)
    }

    public static func ambient(dark isDark: Bool, strength: Float) -> BackdropTone {
        let held = min(max(strength, 0), 1)

        if isDark {
            return BackdropTone(
                lightnessFloor: 0.05,
                lightnessRange: 0.10 + 0.22 * held,
                chroma: 0.5 + 0.5 * held,
                vignette: 0.3
            )
        }
        return BackdropTone(
            lightnessFloor: 0.96 - 0.16 * held,
            lightnessRange: 0.02 + 0.14 * held,
            chroma: 0.3 + 0.5 * held,
            vignette: 0.04
        )
    }
}

public struct FieldBlend: Sendable {
    private static let crossfadeSeconds: TimeInterval = 1.2

    public private(set) var shown: Palette
    private var leaving: Palette
    public private(set) var shownPacked: FieldUniforms.Five
    public private(set) var leavingPacked: FieldUniforms.Five
    private var began = -TimeInterval.infinity

    public init(_ palette: Palette) {
        shown = palette
        leaving = palette
        shownPacked = FieldUniforms.packed(palette)
        leavingPacked = shownPacked
    }

    public mutating func show(_ palette: Palette, at now: TimeInterval, fading: Bool = true) -> Bool {
        guard palette != shown else { return false }
        guard fading else {
            shown = palette
            leaving = palette
            shownPacked = FieldUniforms.packed(palette)
            leavingPacked = shownPacked
            began = -.infinity
            return true
        }
        let reached = Self.eased(progress(at: now))

        leaving = leaving.blended(toward: shown, by: reached)
        shown = palette
        leavingPacked = FieldUniforms.packed(leaving)
        shownPacked = FieldUniforms.packed(shown)
        began = now
        return true
    }

    public func progress(at now: TimeInterval) -> Float {
        Float(min(max((now - began) / Self.crossfadeSeconds, 0), 1))
    }

    public func fading(at now: TimeInterval) -> Bool {
        progress(at: now) < 1
    }

    private static func eased(_ progress: Float) -> Float {
        progress * progress * (3 - 2 * progress)
    }
}

public struct FieldUniforms: Sendable {
    public typealias Five = (SIMD4<Float>, SIMD4<Float>, SIMD4<Float>, SIMD4<Float>, SIMD4<Float>)

    public var colours: Five
    public var previous: Five
    public var time: Float
    public var energy: Float
    public var progress: Float
    public var aspect: Float
    public var lightnessFloor: Float
    public var lightnessRange: Float
    public var chroma: Float
    public var vignette: Float

    public static let stillMoment: Float = 13

    public init(blend: FieldBlend, time: Float, energy: Float, progress: Float, aspect: Float, tone: BackdropTone) {
        colours = blend.shownPacked
        previous = blend.leavingPacked
        self.time = time
        self.energy = energy
        self.progress = progress
        self.aspect = aspect
        lightnessFloor = tone.lightnessFloor
        lightnessRange = tone.lightnessRange
        chroma = tone.chroma
        vignette = tone.vignette
    }

    public static func still(palette: Palette, tone: BackdropTone, aspect: Float) -> FieldUniforms {
        FieldUniforms(blend: FieldBlend(palette), time: stillMoment, energy: 0, progress: 1, aspect: aspect, tone: tone)
    }

    public static func packed(_ palette: Palette) -> Five {
        let packed = zip(palette.colours, palette.weights).map { colour, weight in
            let lab = Oklab.lab(fromLinear: pointwiseMax(colour, .zero))
            return SIMD4<Float>(lab.x, lab.y, lab.z, weight)
        }
        return (packed[0], packed[1], packed[2], packed[3], packed[4])
    }
}
