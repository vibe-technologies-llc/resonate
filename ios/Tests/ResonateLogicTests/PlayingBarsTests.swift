import Foundation
import Testing

@testable import ResonateLogic

@Suite struct PlayingBarsTests {
    private static let window = 256

    private static func tone(atBin bin: Double, amplitude: Float = 1, length: Int = window) -> [Float] {
        (0..<length).map { index in
            amplitude * Float(sin(2 * .pi * bin * Double(index) / Double(length)))
        }
    }

    private static func loudest(_ bands: HeardBands) -> Int? {
        bands.decibels.indices.max { bands.decibels[$0] < bands.decibels[$1] }
    }

    @Test func aToneIsHeardInTheBandHoldingItsFrequency() {
        #expect(Self.loudest(HeardBands(heard: Self.tone(atBin: 2))) == 0)
        #expect(Self.loudest(HeardBands(heard: Self.tone(atBin: 5))) == 1)
        #expect(Self.loudest(HeardBands(heard: Self.tone(atBin: 12))) == 2)
        #expect(Self.loudest(HeardBands(heard: Self.tone(atBin: 60))) == 3)
    }

    @Test func aFullScaleToneReadsNearZeroDecibelsInItsOwnBin() {
        let bands = HeardBands(heard: Self.tone(atBin: 50))
        #expect(bands.decibels[3] < 0)
        #expect(bands.decibels[3] > -25)
    }

    @Test func silenceAndTooFewSamplesAreSilence() {
        #expect(HeardBands(heard: Array(repeating: 0, count: Self.window)) == .silence)
        #expect(HeardBands(heard: []) == .silence)
        #expect(HeardBands(heard: [0.5, -0.5]) == .silence)
    }

    @Test func anotherWindowLengthStillSplitsIntoFourBands() {
        #expect(HeardBands(heard: Self.tone(atBin: 4, length: 512)).decibels.count == HeardBands.count)
        #expect(Self.loudest(HeardBands(heard: Self.tone(atBin: 4, length: 512))) == 0)
        #expect(HeardBands(heard: Self.tone(atBin: 2, length: 32)).decibels.count == HeardBands.count)
    }

    @Test func aBandHeldLoudRisesToTheTopAndSilenceLetsItFallToRest() {
        var bars = PlayingBars()
        let loud = HeardBands(heard: Self.tone(atBin: 2))

        for _ in 0..<20 { bars.hear(loud) }
        #expect(bars.heights[0] > 0.95)

        for _ in 0..<20 { bars.hear(.silence) }
        #expect(bars.heights.allSatisfy { abs($0 - PlayingBars.resting) < 0.01 })
    }

    @Test func theBarsFollowTheMusicWhateverTheVolume() {
        var loud = PlayingBars()
        var quiet = PlayingBars()
        let tones = [2.0, 5, 12, 60]

        for round in 0..<200 {
            let bin = tones[round % tones.count]
            loud.hear(HeardBands(heard: Self.tone(atBin: bin)))
            quiet.hear(HeardBands(heard: Self.tone(atBin: bin, amplitude: 0.05)))
        }

        for (heard, quietly) in zip(loud.heights, quiet.heights) {
            #expect(abs(heard - quietly) < 0.05)
        }
    }

    @Test func aRiseIsQuickerThanAFall() {
        var rising = PlayingBars()
        var falling = PlayingBars()
        let loud = HeardBands(heard: Self.tone(atBin: 2))
        for _ in 0..<20 { falling.hear(loud) }
        for _ in 0..<20 { rising.hear(.silence) }

        let lowBefore = rising.heights[0]
        let highBefore = falling.heights[0]
        rising.hear(loud)
        falling.hear(.silence)

        #expect(rising.heights[0] - lowBefore > highBefore - falling.heights[0])
    }

    @Test func everyHeightStaysBetweenRestAndTheTop() {
        var bars = PlayingBars()
        for round in 0..<100 {
            bars.hear(HeardBands(heard: Self.tone(atBin: Double(round % 90 + 1), amplitude: Float(round % 7) / 6)))
            #expect(bars.heights.allSatisfy { $0 >= PlayingBars.resting - 1e-4 && $0 <= 1 + 1e-4 })
        }
    }

    @Test func restingShowsTheStillBars() {
        var bars = PlayingBars()
        bars.hear(HeardBands(heard: Self.tone(atBin: 2)))
        bars.rest()
        #expect(bars.heights == PlayingBars.still)
    }

    @Test func theBarsStandFourAcrossTheirWidthWithAGapBetween() {
        let shape = PlayingBarsShape(height: 14)
        let lastRightEdge = shape.middle(of: HeardBands.count - 1) + shape.barWidth / 2

        #expect(abs(shape.barWidth - 2.8) < 1e-9)
        #expect(abs(shape.gap - 1.96) < 1e-9)
        #expect(abs(shape.middle(of: 0) - shape.barWidth / 2) < 1e-9)
        #expect(abs(shape.middle(of: 1) - shape.middle(of: 0) - shape.barWidth - shape.gap) < 1e-9)
        #expect(abs(lastRightEdge - shape.width) < 1e-9)
    }

    @Test func aBarIsNeverShorterThanItsWidthNorTallerThanItsRow() {
        let shape = PlayingBarsShape(height: 14)

        #expect(shape.barHeight(0) == shape.barWidth)
        #expect(shape.barHeight(PlayingBars.resting) == shape.barWidth)
        #expect(abs(shape.barHeight(0.5) - 7) < 1e-9)
        #expect(shape.barHeight(1) == shape.height)
    }
}
