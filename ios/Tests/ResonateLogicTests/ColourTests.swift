import Foundation
import ResonateNative
import Testing

@testable import ResonateLogic

@Suite struct AccentMathTests {
    @Test func aHexIsSplitIntoChannelsOfZeroToOne() {
        let rgb = AccentMath.rgb(0xFF8000)

        #expect(rgb.red == 1)
        #expect(abs(rgb.green - 128 / 255) < 1e-9)
        #expect(rgb.blue == 0)
    }

    @Test func inDarkAnAccentThatReadsIsWornAsItIsAndInLightItIsDeepenedByTwentyEightPercentAtLeast() {
        let dark = AccentMath.worn(0x6090C0, dark: true)
        let light = AccentMath.worn(0x6090C0, dark: false)

        #expect(dark.red == AccentMath.rgb(0x6090C0).red)
        #expect(light.red <= dark.red * 0.72 + 1e-9)
        #expect(light.green <= dark.green * 0.72 + 1e-9)
        #expect(light.blue <= dark.blue * 0.72 + 1e-9)
    }

    @Test func contrastIsOneForTheSameColourAndTwentyOneForBlackOnWhite() {
        #expect(AccentMath.contrast(AccentMath.white, AccentMath.white) == 1)
        #expect(abs(AccentMath.contrast(AccentMath.white, (0, 0, 0)) - 21) < 1e-9)
        #expect(AccentMath.contrast((0.2, 0.4, 0.6), AccentMath.white) == AccentMath.contrast(AccentMath.white, (0.2, 0.4, 0.6)))
    }

    @Test(arguments: [true, false], [true, false])
    func everyAccentIsWornAtFourPointFiveToOneAgainstItsGroundAndItsInkAgainstIt(dark: Bool, increased: Bool) {
        let ground = dark ? AccentMath.darkBackground : AccentMath.white
        let wanted = increased ? AccentMath.readableWhenIncreased : AccentMath.readable

        for accent in ResonateAccent.every {
            let worn = AccentMath.worn(accent.hex, dark: dark, increased: increased)

            #expect(AccentMath.contrast(worn, ground) >= wanted - 1e-6, "\(accent.name) dark \(dark) increased \(increased)")
            #expect(AccentMath.inkContrast(on: accent.hex, dark: dark, increased: increased) >= AccentMath.readable, "\(accent.name) ink dark \(dark) increased \(increased)")
        }
    }

    @Test(arguments: [true, false], AccentMath.Statement.allCases)
    func everyStatementInkReadsAtFourPointFiveToOneAgainstItsGround(dark: Bool, statement: AccentMath.Statement) {
        let ground = dark ? AccentMath.darkBackground : AccentMath.white

        #expect(AccentMath.contrast(statement.ink(dark: dark), ground) >= AccentMath.readable, "\(statement) dark \(dark)")
    }

    @Test func deepeningAnAccentInLightKeepsItsHueAndOnlyDarkensIt() {
        for accent in ResonateAccent.every {
            let plain = AccentMath.rgb(accent.hex)
            let worn = AccentMath.worn(accent.hex, dark: false, increased: true)

            #expect(worn.red <= plain.red && worn.green <= plain.green && worn.blue <= plain.blue, "\(accent.name)")
        }
    }

    @Test func luminanceRunsFromBlackToWhiteAndWeighsGreenMost() {
        #expect(AccentMath.luminance(red: 0, green: 0, blue: 0) == 0)
        #expect(abs(AccentMath.luminance(red: 1, green: 1, blue: 1) - 1) < 1e-9)
        #expect(AccentMath.luminance(red: 0, green: 1, blue: 0) > AccentMath.luminance(red: 1, green: 0, blue: 0))
        #expect(AccentMath.luminance(red: 1, green: 0, blue: 0) > AccentMath.luminance(red: 0, green: 0, blue: 1))
    }

    @Test func aDarkAccentIsLetteredInWhiteInLightAndLiftedInDarkSoItIsLetteredInNearBlack() {
        #expect(AccentMath.ink(on: 0x101040, dark: false) == .white)
        #expect(AccentMath.ink(on: 0x101040, dark: true) == .dark(white: AccentMath.darkInkWhite))
        #expect(AccentMath.ink(on: 0xFFE070, dark: true) == .dark(white: AccentMath.darkInkWhite))
    }

    @Test func theSameAccentCanChangeInkBetweenDarkAndLight() {
        #expect(AccentMath.ink(on: 0x60A0FF, dark: true) == .dark(white: AccentMath.darkInkWhite))
        #expect(AccentMath.ink(on: 0x60A0FF, dark: false) == .white)
    }

    @Test(arguments: [true, false])
    func everyAccentIsLetteredInWhicheverInkContrastsMore(dark: Bool) {
        for accent in ResonateAccent.every {
            let worn = AccentMath.worn(accent.hex, dark: dark)
            let luminance = AccentMath.luminance(red: worn.red, green: worn.green, blue: worn.blue)
            let white = (1 + 0.05) / (luminance + 0.05)
            let black = (luminance + 0.05) / (AccentMath.darkInkLuminance + 0.05)

            switch AccentMath.ink(on: accent.hex, dark: dark) {
            case .white: #expect(white >= black, "\(accent.name)")
            case .dark: #expect(black > white, "\(accent.name)")
            }
        }
    }
}

@Suite struct OklabTests {
    @Test func aLinearColourIsReadBackAfterATripThroughLab() {
        for rgb in [SIMD3<Float>(1, 0, 0), SIMD3(0.2, 0.5, 0.8), SIMD3(0, 0, 0), SIMD3(1, 1, 1), SIMD3(0.02, 0.9, 0.3)] {
            let back = Oklab.linear(fromLab: Oklab.lab(fromLinear: rgb))

            #expect(abs(back.x - rgb.x) < 1e-3 && abs(back.y - rgb.y) < 1e-3 && abs(back.z - rgb.z) < 1e-3, "\(rgb)")
        }
    }

    @Test func whiteIsFullLightnessAndBlackNone() {
        #expect(abs(Oklab.lab(fromLinear: SIMD3(1, 1, 1)).x - 1) < 1e-3)
        #expect(abs(Oklab.lab(fromLinear: SIMD3(0, 0, 0)).x) < 1e-6)
    }

    @Test func aGreyHasNoChroma() {
        #expect(Oklab.chroma(of: Oklab.lab(fromLinear: SIMD3(0.3, 0.3, 0.3))) < 1e-3)
        #expect(Oklab.chroma(of: Oklab.lab(fromLinear: SIMD3(0.9, 0.1, 0.1))) > 0.1)
    }

    @Test func polarCoordinatesAreTheirOwnInverse() {
        let lab = Oklab.polar(lightness: 0.5, chroma: 0.2, hue: 1.0)

        #expect(abs(Oklab.chroma(of: lab) - 0.2) < 1e-5)
        #expect(lab.x == 0.5)
        #expect(abs(Float(atan2(Double(lab.z), Double(lab.y))) - 1.0) < 1e-5)
    }
}

@Suite struct PaletteTests {
    @Test func aPaletteAroundAnAccentHasFiveColoursWhoseWeightsSumToOne() {
        let palette = Palette.around(accent: SIMD3(0.1, 0.3, 0.9))

        #expect(palette.colours.count == Palette.size)
        #expect(palette.weights.count == Palette.size)
        #expect(abs(palette.weights.reduce(0, +) - 1) < 1e-5)
    }

    @Test func everyColourOfAPaletteIsInsideTheGamut() {
        for accent in [SIMD3<Float>(1, 0, 0), SIMD3(0, 1, 0), SIMD3(0, 0, 1), SIMD3(0.5, 0.5, 0.5), SIMD3(1, 1, 0)] {
            for colour in Palette.around(accent: accent).colours {
                #expect(colour.min() >= 0 && colour.max() <= 1, "\(accent) \(colour)")
            }
        }
    }

    @Test func clustersAreRankedByMembersAndNoWeightFallsBelowTheSmallest() {
        let clusters: [(centre: SIMD3<Float>, members: Int)] = [
            (SIMD3(0.5, 0.05, 0.05), 1),
            (SIMD3(0.5, -0.05, 0.05), 900),
            (SIMD3(0.5, 0.05, -0.05), 90),
            (SIMD3(0.5, -0.05, -0.05), 9),
            (SIMD3(0.5, 0, 0), 0),
        ]

        let palette = Palette.shaped(clusters, smallestWeight: 0.06)

        #expect(palette.weights.max() == palette.weights[0])
        #expect(palette.weights.min()! > 0.04)
        #expect(abs(palette.weights.reduce(0, +) - 1) < 1e-5)
    }

    @Test func lightnessIsHeldBetweenTheDarkestAndLightestSoACoverNeverMakesBlackOrWhiteField() {
        let dark: [(centre: SIMD3<Float>, members: Int)] = (0..<5).map { (SIMD3(0.01, 0.05, 0.05), 10 - $0) }
        let bright: [(centre: SIMD3<Float>, members: Int)] = (0..<5).map { (SIMD3(0.99, 0.05, 0.05), 10 - $0) }

        let darkField = Palette.shaped(dark, smallestWeight: 0.06)
        let brightField = Palette.shaped(bright, smallestWeight: 0.06)

        for colour in darkField.colours { #expect(Oklab.lab(fromLinear: colour).x >= Palette.darkest - 0.01) }
        for colour in brightField.colours { #expect(Oklab.lab(fromLinear: colour).x <= Palette.lightest + 0.01) }
    }

    @Test func aGreyCoverMayGoDarkerThanAColouredOneAndStaysBelowTheGreyCeiling() {
        let darkGrey: [(centre: SIMD3<Float>, members: Int)] = (0..<5).map { (SIMD3(0.01, 0, 0), 10 - $0) }
        let brightGrey: [(centre: SIMD3<Float>, members: Int)] = (0..<5).map { (SIMD3(0.99, 0, 0), 10 - $0) }

        let darkField = Palette.shaped(darkGrey, smallestWeight: 0.06, darkest: Palette.darkestGrey)
        let brightField = Palette.shaped(brightGrey, smallestWeight: 0.06, darkest: Palette.darkestGrey)

        for colour in darkField.colours { #expect(Oklab.lab(fromLinear: colour).x < Palette.darkest) }
        for colour in brightField.colours { #expect(Oklab.lab(fromLinear: colour).x <= Palette.lightestGrey + 0.01) }
    }

    @Test func samplingEmptyPixelsReturnsNil() {
        #expect(Palette.sampled(from: []) == nil)
    }

    @Test func aMostlyDarkCoverWithAVividAccentKeepsItsDarkDominantGround() throws {
        let dark = lab(red: 10, green: 10, blue: 10)
        let red = lab(red: 240, green: 40, blue: 30)
        let pixels = Array(repeating: dark, count: 80) + Array(repeating: red, count: 20)

        let palette = try #require(Palette.sampled(from: pixels))
        let dominant = Oklab.lab(fromLinear: palette.colours[0])
        let brightest = palette.colours.map { Oklab.lab(fromLinear: $0) }.max { $0.x < $1.x }!

        #expect(dominant.x < Palette.darkest)
        #expect(Oklab.chroma(of: brightest) > 0.08)
        #expect(brightest.x <= Palette.lightest + 0.01)
    }

    @Test func aWhiteBackgroundWithColouredArtworkDropsWhiteGlareToSampleTheArtwork() throws {
        let white = lab(red: 248, green: 246, blue: 242)
        let orange = lab(red: 245, green: 140, blue: 30)
        let pixels = Array(repeating: white, count: 65) + Array(repeating: orange, count: 35)

        let palette = try #require(Palette.sampled(from: pixels))
        let dominant = Oklab.lab(fromLinear: palette.colours[0])

        #expect(Oklab.chroma(of: dominant) > 0.08)
        #expect(dominant.x <= Palette.lightest + 0.01)
    }

    @Test func aPureWhiteCoverStaysBelowTheGreyCeiling() throws {
        let white = lab(red: 248, green: 248, blue: 248)
        let ink = lab(red: 30, green: 30, blue: 30)
        let pixels = Array(repeating: white, count: 95) + Array(repeating: ink, count: 5)

        let palette = try #require(Palette.sampled(from: pixels))

        for colour in palette.colours {
            #expect(Oklab.lab(fromLinear: colour).x <= Palette.lightestGrey + 0.01)
        }
    }

    @Test func aPureSaturatedYellowCoverIsRecognisedAsColouredNotWhiteGlare() throws {
        let yellow = lab(red: 255, green: 255, blue: 0)
        let pixels = Array(repeating: yellow, count: 100)

        let palette = try #require(Palette.sampled(from: pixels))
        let dominant = Oklab.lab(fromLinear: palette.colours[0])

        #expect(Oklab.chroma(of: dominant) > 0.08)
        #expect(dominant.x <= Palette.lightest + 0.01)
    }

    @Test func distinctBrightTonesCompressSmoothlyWithoutCollapsingToOneLightness() throws {
        let mid = lab(red: 150, green: 90, blue: 30)
        let bright = lab(red: 250, green: 215, blue: 60)
        let pixels = Array(repeating: mid, count: 50) + Array(repeating: bright, count: 50)

        let palette = try #require(Palette.sampled(from: pixels))
        let lightnesses = palette.colours.map { Oklab.lab(fromLinear: $0).x }

        #expect(lightnesses.max()! <= Palette.lightest + 0.01)
        #expect(lightnesses.max()! - lightnesses.min()! > 0.02)
    }

    @Test func isolatedBrightSpecksDoNotStealASeedOnADarkCover() throws {
        let navy = lab(red: 12, green: 24, blue: 62)
        let speck = lab(red: 255, green: 255, blue: 255)
        let pixels = Array(repeating: navy, count: 1020) + Array(repeating: speck, count: 4)

        let palette = try #require(Palette.sampled(from: pixels))

        for colour in palette.colours {
            #expect(Oklab.lab(fromLinear: colour).x < 0.35)
        }
    }

    @Test func aSmallRealAccentOnADarkCoverIsStillPickedUp() throws {
        let dark = lab(red: 16, green: 16, blue: 18)
        let green = lab(red: 0, green: 230, blue: 80)
        let pixels = Array(repeating: dark, count: 95) + Array(repeating: green, count: 5)

        let palette = try #require(Palette.sampled(from: pixels))
        let maxChroma = palette.colours.map { Oklab.chroma(of: Oklab.lab(fromLinear: $0)) }.max()!

        #expect(maxChroma > 0.08)
    }

    @Test func everyCoverKeepsComfortableReadabilityAgainstWhiteInDarkAndBlackInLight() throws {
        let covers: [[SIMD3<Float>]] = [
            Array(repeating: lab(red: 8, green: 8, blue: 8), count: 100),
            Array(repeating: lab(red: 10, green: 10, blue: 10), count: 80) + Array(repeating: lab(red: 240, green: 40, blue: 30), count: 20),
            Array(repeating: lab(red: 248, green: 248, blue: 248), count: 95) + Array(repeating: lab(red: 30, green: 30, blue: 30), count: 5),
            Array(repeating: lab(red: 138, green: 206, blue: 0), count: 90) + Array(repeating: lab(red: 10, green: 10, blue: 10), count: 10),
            Array(repeating: lab(red: 255, green: 255, blue: 0), count: 100),
            Array(repeating: lab(red: 0, green: 255, blue: 255), count: 100),
            Array(repeating: lab(red: 245, green: 190, blue: 210), count: 100),
        ]

        for cover in covers {
            let palette = try #require(Palette.sampled(from: cover))
            for colour in palette.colours {
                let darkLuminance = shapedLuminance(colour, tone: .dark)
                let lightLuminance = shapedLuminance(colour, tone: .light)
                let contrastAgainstWhite = (1 + 0.05) / (darkLuminance + 0.05)
                let contrastAgainstBlack = (lightLuminance + 0.05) / 0.05

                #expect(contrastAgainstWhite >= 9.0)
                #expect(contrastAgainstBlack >= 5.0)
            }
        }
    }

    private func lab(red: UInt8, green: UInt8, blue: UInt8) -> SIMD3<Float> {
        func linear(_ channel: UInt8) -> Float {
            let encoded = Float(channel) / 255
            return encoded <= 0.04045 ? encoded / 12.92 : Float(pow((Double(encoded) + 0.055) / 1.055, 2.4))
        }
        return Oklab.lab(fromLinear: SIMD3(linear(red), linear(green), linear(blue)))
    }

    private func shapedLuminance(_ linear: SIMD3<Float>, tone: BackdropTone) -> Float {
        let lab = Oklab.lab(fromLinear: linear)
        let lightness = min(max(lab.x, 0), 1)
        let curved = lightness * 0.65 + lightness * lightness * (3 - 2 * lightness) * 0.35
        let darkened = tone.lightnessFloor + tone.lightnessRange * curved
        let kept = sqrt(min(max(darkened / max(lightness, 0.05), 0), 1))
        let scaled = tone.chroma * kept
        let rgb = Oklab.linear(fromLab: SIMD3(darkened, lab.y * scaled, lab.z * scaled))
            .clamped(lowerBound: SIMD3(repeating: 0), upperBound: SIMD3(repeating: 1))
        return 0.2126 * rgb.x + 0.7152 * rgb.y + 0.0722 * rgb.z
    }

    @Test func blendingMovesEveryColourAndWeightTheShareAsked() {
        let one = Palette.around(accent: SIMD3(0.9, 0.1, 0.1))
        let other = Palette.around(accent: SIMD3(0.1, 0.1, 0.9))

        #expect(one.blended(toward: other, by: 0) == one)
        let whole = one.blended(toward: other, by: 1)
        #expect(zip(whole.colours, other.colours).allSatisfy { abs($0.x - $1.x) < 1e-6 && abs($0.y - $1.y) < 1e-6 })
        #expect(zip(whole.weights, other.weights).allSatisfy { abs($0 - $1) < 1e-6 })
        let half = one.blended(toward: other, by: 0.5)
        #expect(abs(half.colours[0].x - (one.colours[0].x + other.colours[0].x) / 2) < 1e-6)
    }
}

@Suite struct FieldBlendTests {
    let first = Palette.around(accent: SIMD3(0.9, 0.1, 0.1))
    let second = Palette.around(accent: SIMD3(0.1, 0.1, 0.9))
    let third = Palette.around(accent: SIMD3(0.1, 0.9, 0.1))

    @Test func aBlendOfOnePaletteIsAtRestAndNotFading() {
        let blend = FieldBlend(first)

        #expect(blend.progress(at: 0) == 1)
        #expect(!blend.fading(at: 0))
        #expect(blend.shownPacked.0 == blend.leavingPacked.0)
    }

    @Test func theSamePaletteShownAgainChangesNothing() {
        var blend = FieldBlend(first)

        let began1 = blend.show(first, at: 5)

        #expect(!began1)
        #expect(!blend.fading(at: 5))
    }

    @Test func aNewPaletteBeginsACrossfadeOfOneAndATwentiethSeconds() {
        var blend = FieldBlend(first)

        let began2 = blend.show(second, at: 10)

        #expect(began2)

        #expect(blend.progress(at: 10) == 0)
        #expect(abs(blend.progress(at: 10.6) - 0.5) < 1e-6)
        #expect(blend.fading(at: 11.19))
        #expect(!blend.fading(at: 11.2))
        #expect(blend.progress(at: 99) == 1)
        #expect(blend.progress(at: 0) == 0)
    }

    @Test func aPaletteShownWithoutFadingIsAtRestAtOnceEvenMidFade() {
        var blend = FieldBlend(first)
        _ = blend.show(second, at: 10)

        let settled = blend.show(third, at: 10.3, fading: false)

        #expect(settled)
        #expect(!blend.fading(at: 10.3))
        #expect(blend.progress(at: 10.3) == 1)
        #expect(blend.shownPacked.0 == FieldUniforms.packed(third).0)
        #expect(blend.leavingPacked.0 == FieldUniforms.packed(third).0)
    }

    @Test func theShownAndLeavingPalettesArePackedForTheShader() {
        var blend = FieldBlend(first)

        _ = blend.show(second, at: 10)

        #expect(blend.shownPacked.0 == FieldUniforms.packed(second).0)
        #expect(blend.leavingPacked.0 == FieldUniforms.packed(first).0)
    }

    @Test func aChangeMidFadeStartsFromTheBlendThenOnScreenNotFromTheLastPalette() {
        var blend = FieldBlend(first)
        _ = blend.show(second, at: 10)

        _ = blend.show(third, at: 10.6)

        let eased: Float = 0.5 * 0.5 * (3 - 2 * 0.5)
        let onScreen = first.blended(toward: second, by: eased)
        #expect(blend.leavingPacked.0 == FieldUniforms.packed(onScreen).0)
        #expect(blend.shownPacked.0 == FieldUniforms.packed(third).0)
        #expect(blend.progress(at: 10.6) == 0)
    }

    @Test func aChangeAfterTheFadeIsDoneLeavesFromTheLastShownPalette() {
        var blend = FieldBlend(first)
        _ = blend.show(second, at: 10)

        _ = blend.show(third, at: 50)

        #expect(blend.leavingPacked.0 == FieldUniforms.packed(second).0)
    }
}

@Suite struct FieldUniformsTests {
    @Test func aPaletteIsPackedInOklabWithItsWeightAsTheFourthComponent() {
        let palette = Palette.around(accent: SIMD3(0.9, 0.1, 0.1))

        let packed = FieldUniforms.packed(palette)

        #expect(packed.0.w == palette.weights[0])
        #expect(packed.4.w == palette.weights[4])
        #expect(abs(packed.0.x - Oklab.lab(fromLinear: palette.colours[0]).x) < 1e-6)
    }

    @Test func theUniformsAreAsBigAsTheShadersFieldReadsAndTheFieldIsWhatTheShaderDeclares() throws {
        let declared = try shaderField()

        #expect(MemoryLayout<FieldUniforms>.stride == 192)
        #expect(MemoryLayout<FieldUniforms>.stride == declared.bytes)
        #expect(
            declared.names == [
                "colours", "previous", "time", "energy", "progress", "aspect", "lightnessFloor", "lightnessRange", "chroma",
                "vignette",
            ]
        )
    }

    @Test func theFieldsAreInTheOrderTheShaderReadsThem() {
        let blend = FieldBlend(Palette.around(accent: SIMD3(0.5, 0.5, 0.5)))
        let uniforms = FieldUniforms(blend: blend, time: 1, energy: 2, progress: 3, aspect: 4, tone: BackdropTone(lightnessFloor: 5, lightnessRange: 6, chroma: 7, vignette: 8))

        let floats = withUnsafeBytes(of: uniforms) { Array($0.bindMemory(to: Float.self)) }

        #expect(Array(floats[40...]) == [1, 2, 3, 4, 5, 6, 7, 8])
    }

    @Test func aStillFieldIsDrawnAtThePlacedMomentWithNoEnergyAndAllTheWayThrough() {
        let still = FieldUniforms.still(palette: Palette.around(accent: SIMD3(0.5, 0.5, 0.5)), tone: .dark, aspect: 2)

        #expect(still.time == FieldUniforms.stillMoment)
        #expect(still.energy == 0)
        #expect(still.progress == 1)
        #expect(still.aspect == 2)
    }

    @Test func theShaderSourceDeclaresNothingWithASwiftAccessLevel() {
        let words = BackdropShader.source.split(whereSeparator: { !$0.isLetter && !$0.isNumber && $0 != "_" })

        #expect(!words.contains { ["public", "private", "fileprivate", "internal", "open"].contains($0) })
    }

    func shaderField() throws -> (bytes: Int, names: [String]) {
        let source = BackdropShader.source
        let opening = try #require(source.range(of: "struct Field {"))
        let closing = try #require(source.range(of: "};", range: opening.upperBound..<source.endIndex))
        let body = source[opening.upperBound..<closing.lowerBound]

        var bytes = 0
        var names: [String] = []
        for line in body.split(separator: ";") {
            let words = line.split(whereSeparator: { $0.isWhitespace })
            guard words.count == 2 else { continue }
            let (type, name) = (String(words[0]), String(words[1]))
            let count = name.firstIndex(of: "[").flatMap { Int(name[name.index(after: $0)..<name.firstIndex(of: "]")!]) } ?? 1
            let size = ["float4": 16, "float": 4][type] ?? 0
            #expect(size > 0, "the shader's Field holds a \(type) this test cannot size")
            bytes += size * count
            names.append(String(name.prefix { $0 != "[" }))
        }
        return (bytes, names)
    }
}

@Suite struct BackdropToneTests {
    @Test func theAmbientToneInDarkRisesFromItsFloorWithStrength() {
        let none = BackdropTone.ambient(dark: true, strength: 0)
        let full = BackdropTone.ambient(dark: true, strength: 1)

        #expect(none.lightnessFloor == 0.05 && full.lightnessFloor == 0.05)
        #expect(abs(none.lightnessRange - 0.10) < 1e-6)
        #expect(abs(full.lightnessRange - 0.32) < 1e-6)
        #expect(abs(none.chroma - 0.5) < 1e-6 && abs(full.chroma - 1) < 1e-6)
    }

    @Test func theAmbientToneInLightFallsFromAlmostWhiteWithStrength() {
        let none = BackdropTone.ambient(dark: false, strength: 0)
        let full = BackdropTone.ambient(dark: false, strength: 1)

        #expect(abs(none.lightnessFloor - 0.96) < 1e-6)
        #expect(abs(full.lightnessFloor - 0.80) < 1e-6)
        #expect(full.lightnessRange > none.lightnessRange)
    }

    @Test func theMiddleIntensityIsTheToneAsItWas() {
        #expect(BackdropTone.nowPlaying(dark: true, intensity: 0.5) == BackdropTone.dark)
        #expect(BackdropTone.nowPlaying(dark: false, intensity: 0.5) == BackdropTone.light)
    }

    @Test func aLowerIntensityIsDarkerAndDullerWithAHeavierVignette() {
        for isDark in [true, false] {
            let base = BackdropTone.nowPlaying(dark: isDark, intensity: 0.5)
            let dimmed = BackdropTone.nowPlaying(dark: isDark, intensity: 0)

            #expect(dimmed.lightnessFloor + dimmed.lightnessRange < base.lightnessFloor + base.lightnessRange)
            #expect(dimmed.chroma < base.chroma)
            #expect(dimmed.vignette > base.vignette)
        }
    }

    @Test func aHigherIntensityIsBrighterAndRicherWithALighterVignette() {
        for isDark in [true, false] {
            let base = BackdropTone.nowPlaying(dark: isDark, intensity: 0.5)
            let vivid = BackdropTone.nowPlaying(dark: isDark, intensity: 1)

            #expect(vivid.lightnessFloor + vivid.lightnessRange > base.lightnessFloor + base.lightnessRange)
            #expect(vivid.chroma > base.chroma)
            #expect(vivid.vignette < base.vignette)
        }
    }

    @Test func anIntensityOutsideZeroToOneIsHeldToIt() {
        #expect(BackdropTone.nowPlaying(dark: true, intensity: 9) == BackdropTone.nowPlaying(dark: true, intensity: 1))
        #expect(BackdropTone.nowPlaying(dark: true, intensity: -9) == BackdropTone.nowPlaying(dark: true, intensity: 0))
    }

    @Test func aStrengthOutsideZeroToOneIsHeldToIt() {
        #expect(BackdropTone.ambient(dark: true, strength: 7) == BackdropTone.ambient(dark: true, strength: 1))
        #expect(BackdropTone.ambient(dark: false, strength: -3) == BackdropTone.ambient(dark: false, strength: 0))
    }
}

@Suite struct WrittenPositionTests {
    @Test func aPlayingRatePutsTheExpectedPositionOnAtTheSpeedOfTheClock() {
        let written = WrittenPosition(positionMs: 10_000, at: .now, rate: 1)

        #expect(abs(written.expected(at: written.at + .seconds(2)) - 12_000) < 1e-6)
        #expect(abs(written.expected(at: written.at + .milliseconds(250)) - 10_250) < 1e-6)
    }

    @Test func aPausedRateKeepsItWhereItWas() {
        let written = WrittenPosition(positionMs: 10_000, at: .now, rate: 0)

        #expect(written.expected(at: written.at + .seconds(30)) == 10_000)
    }

    @Test func aMomentBeforeTheWriteIsBeforeThePosition() {
        let written = WrittenPosition(positionMs: 10_000, at: .now, rate: 1)

        #expect(written.expected(at: written.at - .seconds(1)) < 10_000)
    }
}
