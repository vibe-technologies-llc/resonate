import Foundation
import ResonateNative
import Testing

@testable import ResonateLogic

func band(
    _ filter: EqualiserFilter = .peaking,
    hertz: Float = 1_000,
    gain: Float = 0,
    q: Float = 1
) -> EqualiserBand {
    EqualiserBand(filter: filter, frequencyHz: hertz, gainDb: gain, q: q)
}

@Suite struct EqualiserScaleTests {
    let scale = EqualiserScale(size: CGSize(width: 1_000, height: 300), span: 15)

    @Test func theLowestFrequencyIsTheLeftEdgeAndTheHighestTheRight() {
        #expect(scale.x(20) == 0)
        #expect(abs(scale.x(20_000) - 1_000) < 1e-9)
    }

    @Test func eachDecadeTakesAThirdOfTheWidth() {
        #expect(abs(scale.x(200) - 1_000 / 3) < 1e-2)
        #expect(abs(scale.x(2_000) - 2_000 / 3) < 1e-2)
    }

    @Test func aPositionIsReadBackToItsFrequency() {
        for hertz: Float in [20, 100, 1_000, 8_000, 20_000] {
            #expect(abs(scale.hertz(atX: scale.x(hertz)) - hertz) / hertz < 1e-3)
        }
    }

    @Test func aPositionOutsideTheGraphIsHeldToItsEdges() {
        #expect(scale.hertz(atX: -50) == 20)
        #expect(abs(scale.hertz(atX: 5_000) - 20_000) < 1)
    }

    @Test func theTopOfTheGraphIsThePlusSpanAndTheBottomMinus() {
        #expect(scale.y(15) == 0)
        #expect(scale.y(-15) == 300)
        #expect(scale.y(0) == 150)
    }

    @Test func aLevelBeyondTheSpanIsDrawnAtItsEdge() {
        #expect(scale.y(40) == 0)
        #expect(scale.y(-40) == 300)
    }

    @Test func aHeightIsReadBackToItsLevel() {
        for decibels: Float in [-15, -7.5, 0, 3, 15] {
            #expect(abs(scale.decibels(atY: scale.y(decibels)) - decibels) < 1e-4)
        }
    }

    @Test func aGraphOfNoSizeDoesNotDivideByZero() {
        let none = EqualiserScale(size: .zero, span: 15)

        #expect(none.hertz(atX: 0).isFinite)
        #expect(none.decibels(atY: 0).isFinite)
        #expect(none.x(1_000) == 0)
        #expect(none.y(3) == 0)
    }

    @Test func theSpanHoldsTheLoudestOfTheResponseAndTheHandlesInWholeMarks() {
        #expect(EqualiserScale.span(around: [], handles: []) == EqualiserScale.narrowestSpan)
        #expect(EqualiserScale.span(around: [3, -9], handles: [4]) == 15)
        #expect(EqualiserScale.span(around: [16.2], handles: []) == 20)
        #expect(EqualiserScale.span(around: [-22], handles: [3]) == 25)
        #expect(EqualiserScale.span(around: [90], handles: []) == EqualiserScale.widestSpan)
    }

    @Test func aResponseOrHandleThatIsNotANumberDoesNotWidenTheSpan() {
        #expect(EqualiserScale.span(around: [.nan, .infinity, 2], handles: [-.infinity]) == EqualiserScale.narrowestSpan)
    }
}

@Suite struct EqualiserCurveTests {
    @Test func aBandThatUsesGainSitsAtItsGainLiftedByThePreamp() {
        let curve = EqualiserCurve(name: "", preampDb: -4, bands: [])

        #expect(curve.level(of: band(.peaking, gain: 6)) == 2)
        #expect(curve.level(of: band(.highShelf, gain: -3)) == -7)
    }

    @Test func aBandWithoutGainSitsAtThePreamp() {
        let curve = EqualiserCurve(name: "", preampDb: -4, bands: [])

        #expect(curve.level(of: band(.lowPass, gain: 6)) == -4)
        #expect(curve.level(of: band(.notch, gain: 6)) == -4)
    }

    @Test func eachFilterSaysWhetherItUsesGain() {
        #expect([EqualiserFilter.peaking, .lowShelf, .highShelf].allSatisfy { $0.usesGain })
        #expect([EqualiserFilter.lowPass, .highPass, .notch, .bandPass, .allPass].allSatisfy { !$0.usesGain })
        #expect(EqualiserFilter.every.count == 8)
        #expect(Set(EqualiserFilter.every.map(\.label)).count == 8)
    }

    @Test func aCurveIsSummarisedByItsNameOrByWhatItIs() {
        #expect(EqualiserCurve(name: "My headphones", preampDb: 0, bands: []).summary == "My headphones")
        #expect(EqualiserCurve.flat.summary == "Flat")
        #expect(EqualiserCurve(name: "", preampDb: 0, bands: [band(), band()]).summary == "Custom curve · 2 bands")
        #expect(EqualiserCurve(name: "", preampDb: 0, bands: [band()]).summary == "Custom curve · 1 band")
    }

    @Test func aCurveWithNoPreampAndOnlyFlatGainBandsIsCalledFlat() {
        #expect(EqualiserCurve(name: "x", preampDb: 0, bands: [band(gain: 0), band(.notch, gain: 7)]).presetName == "x")
        #expect(EqualiserCurve(name: "x", preampDb: 0, bands: [band(gain: 0), band(.lowShelf, gain: 0)]).presetName == "Flat")
        #expect(EqualiserCurve(name: "x", preampDb: -1, bands: []).presetName == "x")
        #expect(EqualiserCurve(name: "x", preampDb: 0, bands: [band(gain: 3)]).presetName == "x")
    }

    @Test func replacingABandChangesThatOneAndAnIndexOutOfRangeChangesNothing() {
        let curve = EqualiserCurve(name: "", preampDb: 0, bands: [band(hertz: 100), band(hertz: 200)])

        #expect(curve.replacing(1, with: band(hertz: 999)).bands.map(\.frequencyHz) == [100, 999])
        #expect(curve.replacing(2, with: band(hertz: 999)) == curve)
        #expect(curve.replacing(-1, with: band(hertz: 999)) == curve)
    }

    @Test func aGainAndAQAreTidiedToTheirSteps() {
        #expect(EqualiserUnits.tidy(decibels: 2.349) == 2.3)
        #expect(EqualiserUnits.tidy(decibels: -2.351) == -2.4)
        #expect(EqualiserUnits.tidy(q: 1.4142135) == 1.41)
    }
}

@Suite struct EqualiserUnitsTests {
    @Test func aFrequencyIsWrittenToThePrecisionItsSizeNeeds() {
        #expect(EqualiserUnits.hertz(31.25) == "31.2 Hz")
        #expect(EqualiserUnits.hertz(125) == "125 Hz")
        #expect(EqualiserUnits.hertz(1_000) == "1.00 kHz")
        #expect(EqualiserUnits.hertz(2_500) == "2.50 kHz")
        #expect(EqualiserUnits.hertz(16_000) == "16.0 kHz")
    }

    @Test func aGainIsSignedToOneDecimal() {
        #expect(EqualiserUnits.decibels(0) == "+0.0 dB")
        #expect(EqualiserUnits.decibels(3.25) == "+3.2 dB" || EqualiserUnits.decibels(3.25) == "+3.3 dB")
        #expect(EqualiserUnits.decibels(-12) == "-12.0 dB")
    }

    @Test(arguments: [
        (Float(31.25), "Sub-bass"), (62.5, "Bass"), (125, "Bass"), (250, "Warmth"), (500, "Low mids"), (1_000, "Mids"),
        (2_000, "Vocals"), (4_000, "Presence"), (8_000, "Treble"), (16_000, "Air"),
    ])
    func aFrequencyIsNamedByTheRangeItFallsIn(hertz: Float, name: String) {
        #expect(EqualiserUnits.range(of: hertz) == name)
    }

    @Test func aQIsWrittenToTwoDecimals() {
        #expect(EqualiserUnits.q(1.4142135) == "Q 1.41")
    }

    @Test func aFrequencyIsTidiedToWholeStepsByDecade() {
        #expect(EqualiserUnits.tidy(hertz: 31.26) == 31.3)
        #expect(EqualiserUnits.tidy(hertz: 123.6) == 124)
        #expect(EqualiserUnits.tidy(hertz: 1_234) == 1_230)
    }
}

actor Traces {
    private(set) var asked: [(EqualiserCurve, UInt32?)] = []
    private(set) var running = 0
    private(set) var mostRunning = 0

    func trace(_ curve: EqualiserCurve, _ rate: UInt32?, takes: Duration, answering: [Float]) async -> [Float] {
        asked.append((curve, rate))
        running += 1
        mostRunning = max(mostRunning, running)
        try? await Task.sleep(for: takes)
        running -= 1
        return answering
    }
}

@MainActor @Suite struct EqualiserResponseTests {
    let traces = Traces()

    func curve(_ preamp: Float) -> EqualiserCurve {
        EqualiserCurve(name: "", preampDb: preamp, bands: [])
    }

    func response(takes: Duration = .milliseconds(5)) -> EqualiserResponse {
        let traces = traces
        return EqualiserResponse(
            tracing: { curve, rate in await traces.trace(curve, rate, takes: takes, answering: [curve.preampDb, Float(rate ?? 0)]) },
            between: .milliseconds(15)
        )
    }

    @Test func aCurveFollowedIsTracedAndItsDecibelsAreHeld() async {
        let response = response()

        response.follow(curve(3), rate: 48_000)
        await eventually { !response.decibels.isEmpty }

        #expect(response.decibels == [3, 48_000])
        #expect(await traces.asked.count == 1)
    }

    @Test func aCurveAskedWhileOneIsBeingTracedWaitsAndOnlyTheLatestIsTraced() async {
        let response = response(takes: .milliseconds(60))

        response.follow(curve(1), rate: nil)
        try? await Task.sleep(for: .milliseconds(10))
        response.follow(curve(2), rate: nil)
        response.follow(curve(3), rate: nil)
        response.follow(curve(4), rate: nil)
        await eventually { response.decibels.first == 4 }

        #expect(await traces.asked.map(\.0.preampDb) == [1, 4])
        #expect(await traces.mostRunning == 1)
    }

    @Test func theSameCurveFollowedAgainOnceDrawnAsksNothingMore() async {
        let response = response()
        response.follow(curve(3), rate: 48_000)
        await eventually { !response.decibels.isEmpty }

        response.follow(curve(3), rate: 48_000)
        try? await Task.sleep(for: .milliseconds(80))

        #expect(await traces.asked.count == 1)
    }

    @Test func aStepBackToTheDrawnCurveWhileAnotherIsBeingTracedIsTracedOnceThatOneLands() async {
        let response = response(takes: .milliseconds(40))
        response.follow(curve(1), rate: nil)
        await eventually { !response.decibels.isEmpty }

        response.follow(curve(2), rate: nil)
        try? await Task.sleep(for: .milliseconds(5))
        response.follow(curve(1), rate: nil)
        await eventually { await traces.asked.count == 3 }
        try? await Task.sleep(for: .milliseconds(100))

        #expect(await traces.asked.map(\.0.preampDb) == [1, 2, 1])
        #expect(response.decibels.first == 1)
    }

    @Test func aTraceThatFailsLeavesTheLastDrawingAndIsAskedAgainByTheNextFollow() async {
        let failing = Failing()
        let response = EqualiserResponse(tracing: { curve, _ in
            if await failing.shouldFail() { throw Refusal() }
            return [curve.preampDb]
        })

        response.follow(curve(1), rate: nil)
        try? await Task.sleep(for: .milliseconds(60))
        #expect(response.decibels.isEmpty)

        response.follow(curve(1), rate: nil)
        await eventually { !response.decibels.isEmpty }
        #expect(response.decibels == [1])
    }

    @Test func asksArePacedByTheGapBetweenTraces() async {
        let response = response(takes: .milliseconds(1))
        let began = ContinuousClock.now

        response.follow(curve(1), rate: nil)
        try? await Task.sleep(for: .milliseconds(2))
        response.follow(curve(2), rate: nil)
        await eventually { response.decibels.first == 2 }

        #expect(ContinuousClock.now - began >= .milliseconds(15))
    }
}

actor Failing {
    private var failed = false

    func shouldFail() -> Bool {
        defer { failed = true }
        return !failed
    }
}

@Suite struct SettingsSearchTests {
    @Test func aQuerySplitsIntoItsWordsOnAnyWhitespace() {
        #expect(SettingsSearch.terms("  online\tfeatures\n internet ") == ["online", "features", "internet"])
        #expect(SettingsSearch.terms("   ").isEmpty)
        #expect(SettingsSearch.terms("").isEmpty)
    }

    @Test func everyWordMustBeInTheEntryInAnyOrderAndAnyCase() {
        let searched = "Sound Output Buffer, bit-perfect and sample rate"

        #expect(SettingsSearch.matches(searched, terms: ["BUFFER", "rate"]))
        #expect(SettingsSearch.matches(searched, terms: ["rate", "sound"]))
        #expect(!SettingsSearch.matches(searched, terms: ["buffer", "lyrics"]))
    }

    @Test func aWordMatchesInsideAWordToo() {
        #expect(SettingsSearch.matches("Scrobbling ListenBrainz", terms: ["brainz"]))
    }

    @Test func noWordsMatchEverything() {
        #expect(SettingsSearch.matches("anything", terms: []))
    }

    @Test func accentsAreFoldedByTheComparison() {
        #expect(SettingsSearch.matches("Björk", terms: ["BJÖRK"]))
    }
}

@Suite struct RowMovesTests {
    @Test func aQueueMoveIsOffsetByWhereTheShownPartStarts() {
        let move = RowMoves.queue(offset: 2, destination: 5, within: 10)

        #expect(move.from == 12)
        #expect(move.to == 15)
    }

    @Test func aQueueDestinationIsPassedThroughAsSwiftUIGivesItBeforeTheMove() {
        let move = RowMoves.queue(offset: 4, destination: 0, within: 0)

        #expect(move.from == 4)
        #expect(move.to == 0)
    }

    @Test func aJoinedMoveSkipsTheHeadingBetweenTheTwoRuns() {
        let down = RowMoves.joined(offset: 4, destination: 0, joint: 2, within: 10)
        let up = RowMoves.joined(offset: 0, destination: 5, joint: 2, within: 10)

        #expect(down?.from == 13 && down?.to == 10)
        #expect(up?.from == 10 && up?.to == 14)
    }

    @Test func aJoinedDestinationAtEitherSideOfTheHeadingIsTheSameQueuePlace() {
        let above = RowMoves.joined(offset: 4, destination: 2, joint: 2, within: 0)
        let below = RowMoves.joined(offset: 4, destination: 3, joint: 2, within: 0)

        #expect(above?.to == 2 && below?.to == 2)
    }

    @Test func theHeadingItselfIsNeverMoved() {
        #expect(RowMoves.joined(offset: 2, destination: 0, joint: 2, within: 0) == nil)
    }

    @Test func aPlaylistMoveDownwardLandsOneEarlierBecauseTheRowLeavesFirst() {
        #expect(RowMoves.playlist(from: 2, destination: 5) == 4)
        #expect(RowMoves.playlist(from: 0, destination: 1) == 0)
    }

    @Test func aPlaylistMoveUpwardLandsWhereTheDestinationSays() {
        #expect(RowMoves.playlist(from: 5, destination: 2) == 2)
        #expect(RowMoves.playlist(from: 5, destination: 0) == 0)
    }

    @Test func droppingARowBackWhereItWasIsNoMove() {
        #expect(RowMoves.playlist(from: 3, destination: 3) == 3)
        #expect(RowMoves.playlist(from: 3, destination: 4) == 3)
    }

    @Test func movingARowToTheEndOfAnNRowListLandsOnTheLastPlace() {
        #expect(RowMoves.playlist(from: 1, destination: 6) == 5)
    }
}
