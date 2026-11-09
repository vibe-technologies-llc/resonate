import Foundation
import ResonateNative
import Testing

@testable import ResonateLogic

let phoneLimits = EqualiserLimits(
    bandsAtMost: 32, lowestHz: 20, highestHz: 20_000, gainAtMostDb: 24, preampAtMostDb: 24, qAtLeast: 0.1, qAtMost: 40
)

func curve(_ bands: [EqualiserBand], preamp: Float = 0, name: String = "") -> EqualiserCurve {
    EqualiserCurve(name: name, preampDb: preamp, bands: bands)
}

@Suite struct EqualiserShownTests {
    let octaves = tenBandCurve(preampDb: 0, gainsDb: [])

    @Test func aCurveWithNoBandsShowsTheTenOctaveHandlesAtItsOwnPreamp() {
        let shown = EqualiserShown.curve(curve([], preamp: -3), octaves: octaves)

        #expect(shown.bands.count == 10)
        #expect(shown.bands.allSatisfy { $0.gainDb == 0 })
        #expect(shown.preampDb == -3)
    }

    @Test func aCurveWithBandsIsShownAsItIs() {
        let held = curve([band(hertz: 440, gain: 2)], preamp: -1, name: "Mine")

        #expect(EqualiserShown.curve(held, octaves: octaves) == held)
    }
}

@Suite struct BandSelectionTests {
    @Test func theBandChosenIsShownWhileItIsThere() {
        #expect(BandSelection.shown(3, among: 5) == 3)
    }

    @Test func withNothingChosenOrABandGoneTheFirstIsShown() {
        #expect(BandSelection.shown(nil, among: 5) == 0)
        #expect(BandSelection.shown(7, among: 5) == 0)
        #expect(BandSelection.shown(-1, among: 5) == 0)
    }

    @Test func aCurveWithoutBandsShowsNoBand() {
        #expect(BandSelection.shown(0, among: 0) == nil)
    }

    @Test func removingABandChoosesTheOneThatTookItsPlaceOrTheLast() {
        #expect(BandSelection.afterRemoving(2, leaving: 4) == 2)
        #expect(BandSelection.afterRemoving(4, leaving: 4) == 3)
        #expect(BandSelection.afterRemoving(0, leaving: 0) == nil)
    }
}

@Suite struct BandPlacementTests {
    @Test func aBandAddedToAnEmptyCurveSitsAtOneKilohertz() {
        let added = BandPlacement.added(to: curve([]), limits: phoneLimits)

        #expect(added?.frequencyHz == 1_000)
        #expect(added?.filter == .peaking)
        #expect(added?.gainDb == 0)
        #expect(added?.q == EqualiserUnits.addedQ)
    }

    @Test func aBandIsAddedInTheMiddleOfTheWidestGap() {
        let hertz = BandPlacement.freeHertz(among: [100, 200, 6_400], lowest: 20, highest: 20_000)

        #expect(abs(hertz - 1_130) < 1)
    }

    @Test func amongEqualGapsTheOneNearestOneKilohertzIsTakenThenTheLower() {
        let octaves = tenBandCurve(preampDb: 0, gainsDb: []).bands.map(\.frequencyHz)

        #expect(BandPlacement.freeHertz(among: octaves, lowest: 20, highest: 20_000) == 707)
    }

    @Test func noBandIsAddedPastTheMost() {
        let full = curve(Array(repeating: band(), count: 32))

        #expect(BandPlacement.added(to: full, limits: phoneLimits) == nil)
    }
}

@Suite struct EqualiserHandlesTests {
    let scale = EqualiserScale(size: CGSize(width: 300, height: 200), span: 15)

    @Test func eachHandleSitsOnItsBandLiftedByThePreampAndReachesAtLeastFortyFourPoints() {
        let shaped = curve([band(hertz: 1_000, gain: 3), band(.lowPass, hertz: 8_000)], preamp: -3)
        let handles = EqualiserHandles.laid(shaped, on: scale, selected: nil)

        #expect(handles.map(\.band) == [0, 1])
        #expect(handles[0].centre == CGPoint(x: scale.x(1_000), y: scale.y(0)))
        #expect(handles[1].centre.y == scale.y(-3))
        #expect(handles.allSatisfy { $0.reach.width >= 44 && $0.reach.height >= 44 })
        #expect(handles.allSatisfy { abs($0.reach.midX - $0.centre.x) < 1e-9 && abs($0.reach.midY - $0.centre.y) < 1e-9 })
    }

    @Test func aTouchOffEveryHandleTakesNoneSoTheScrollHasIt() {
        let handles = EqualiserHandles.laid(curve([band(hertz: 1_000)]), on: scale, selected: nil)

        #expect(EqualiserHandles.hit(CGPoint(x: 5, y: 5), among: handles) == nil)
        #expect(EqualiserHandles.hit(handles[0].centre, among: handles) == 0)
    }

    @Test func whereTwoHandlesOverlapTheChosenOneIsOnTopElseTheLater() {
        let close = curve([band(hertz: 1_000), band(hertz: 1_100)])
        let between = CGPoint(x: (scale.x(1_000) + scale.x(1_100)) / 2, y: scale.y(0))

        #expect(EqualiserHandles.hit(between, among: EqualiserHandles.laid(close, on: scale, selected: nil)) == 1)
        #expect(EqualiserHandles.hit(between, among: EqualiserHandles.laid(close, on: scale, selected: 0)) == 0)
    }
}

@Suite struct EqualiserDragTests {
    let scale = EqualiserScale(size: CGSize(width: 300, height: 300), span: 15)

    @Test func dragDownLowersTheGainAndSidewaysMovesTheFrequency() {
        let start = CGPoint(x: scale.x(1_000), y: scale.y(0))
        let moved = EqualiserDrag.moved(
            band(hertz: 1_000), from: start, by: CGSize(width: 100, height: 50), on: scale, preamp: 0, limits: phoneLimits
        )

        #expect(moved.gainDb == -5)
        #expect(abs(moved.frequencyHz - 10_000) < 10)
    }

    @Test func theGainIsReadAboveThePreamp() {
        let start = CGPoint(x: scale.x(1_000), y: scale.y(-4))
        let moved = EqualiserDrag.moved(
            band(hertz: 1_000), from: start, by: CGSize(width: 0, height: -60), on: scale, preamp: -4, limits: phoneLimits
        )

        #expect(moved.gainDb == 6)
    }

    @Test func aBandWithoutGainOnlyMovesSideways() {
        let start = CGPoint(x: scale.x(1_000), y: scale.y(0))
        let moved = EqualiserDrag.moved(
            band(.highPass, hertz: 1_000), from: start, by: CGSize(width: -100, height: 80), on: scale, preamp: 0, limits: phoneLimits
        )

        #expect(moved.gainDb == 0)
        #expect(abs(moved.frequencyHz - 100) < 1)
    }

    @Test func aDragPastTheEdgesIsHeldToTheLimitsNotToTheDrawnSpan() {
        let start = CGPoint(x: scale.x(1_000), y: scale.y(0))
        let moved = EqualiserDrag.moved(
            band(hertz: 1_000), from: start, by: CGSize(width: 5_000, height: -5_000), on: scale, preamp: 0, limits: phoneLimits
        )

        #expect(moved.frequencyHz == 20_000)
        #expect(moved.gainDb == 24)
    }

    @Test func aTouchThatHardlyMovesIsATap() {
        #expect(!EqualiserDrag.travelled(CGSize(width: 2, height: 3)))
        #expect(EqualiserDrag.travelled(CGSize(width: 0, height: 6)))
    }
}

@Suite struct BandAxisTests {
    @Test func theGainShowsTwelveDecibelsEachWayUnlessTheBandReachesFurther() {
        #expect(BandAxis.gain(around: 3, limits: phoneLimits).highest == 12)
        #expect(BandAxis.gain(around: -14, limits: phoneLimits).lowest == -18)
        #expect(BandAxis.gain(around: 30, limits: phoneLimits).highest == 24)
    }

    @Test func aGainSlidesEvenlyAndIsTidiedToATenthOfADecibel() {
        let axis = BandAxis.gain(around: 0, limits: phoneLimits)

        #expect(axis.position(of: 0) == 0.5)
        #expect(axis.value(at: 0.75) == 6)
        #expect(axis.value(at: 0.501) == 0)
        #expect(axis.value(at: 2) == 12)
    }

    @Test func aFrequencySlidesByOctavesNotHertz() {
        let axis = BandAxis.frequency(phoneLimits)

        #expect(abs(axis.position(of: 632.5) - 0.5) < 1e-3)
        #expect(axis.value(at: 0) == 20)
        #expect(axis.value(at: 1) == 20_000)
        #expect(axis.value(at: 0.5) == 632)
    }

    @Test func aWidthSlidesLogarithmicallyBetweenItsBounds() {
        let axis = BandAxis.width(phoneLimits)

        #expect(axis.value(at: 0) == 0.1)
        #expect(axis.value(at: 1) == 40)
        #expect(abs(axis.position(of: 2) - Double(log(2 / 0.1) / log(40 / 0.1))) < 1e-6)
    }

    @Test func aStepIsHalfADecibelASemitoneOrASixthOfAnOctaveOfQ() {
        #expect(BandAxis.gain(around: 0, limits: phoneLimits).stepped(1, up: true) == 1.5)
        #expect(BandAxis.frequency(phoneLimits).stepped(1_000, up: true) == 1_060)
        #expect(BandAxis.width(phoneLimits).stepped(1, up: false) == 0.89)
        #expect(BandAxis.gain(around: 0, limits: phoneLimits).stepped(24, up: true) == 24)
    }

    @Test func eachValueIsWrittenAndSpokenInItsUnit() {
        #expect(BandAxis.gain(around: 0, limits: phoneLimits).written(3.5) == "+3.5 dB")
        #expect(BandAxis.frequency(phoneLimits).written(1_250) == "1.25 kHz")
        #expect(BandAxis.width(phoneLimits).written(1.41) == "Q 1.41")
        #expect(BandAxis.preamp(around: 0, limits: phoneLimits).spoken(-6) == "-6.0 decibels")
        #expect(BandAxis.frequency(phoneLimits).spoken(250) == "250 hertz")
        #expect(BandAxis.frequency(phoneLimits).spoken(2_500) == "2.50 kilohertz")
    }

    @Test func anExactValueIsAskedForByItsBandAndMeasureWithTheWholeRange() {
        #expect(BandAxis.gain(around: 0, limits: phoneLimits).entering(band: 3) == "Band 3 gain")
        #expect(BandAxis.frequency(phoneLimits).entering(band: 1) == "Band 1 frequency")
        #expect(BandAxis.width(phoneLimits).entering(band: 2) == "Band 2 width")
        #expect(BandAxis.preamp(around: 0, limits: phoneLimits).entering(band: nil) == "Preamp level")
        #expect(BandAxis.gain(around: 0, limits: phoneLimits).range == "From -24.0 dB to +24.0 dB")
        #expect(BandMeasure.preamp.title == "Level")
    }

    @Test func aFieldOpensHoldingTheValueWithNoUnit() {
        #expect(BandAxis.gain(around: 0, limits: phoneLimits).typing(-3) == "-3")
        #expect(BandAxis.frequency(phoneLimits).typing(1_230) == "1230")
        #expect(BandAxis.width(phoneLimits).typing(1.41) == "1.41")
    }

    @Test func aTypedValueIsReadWithOrWithoutItsUnit() {
        let gain = BandAxis.gain(around: 0, limits: phoneLimits)
        let frequency = BandAxis.frequency(phoneLimits)
        let width = BandAxis.width(phoneLimits)

        #expect(gain.read("+3.5 dB") == 3.5)
        #expect(gain.read("\u{2212}2") == -2)
        #expect(gain.read("1,5") == 1.5)
        #expect(frequency.read("1.5 kHz") == 1_500)
        #expect(frequency.read("2k") == 2_000)
        #expect(frequency.read("1,000") == 1_000)
        #expect(frequency.read("440 Hz") == 440)
        #expect(width.read("Q 2") == 2)
    }

    @Test func aTypedGainBeyondWhatTheSliderShowsIsTakenUpToTheLimit() {
        let gain = BandAxis.gain(around: 0, limits: phoneLimits)

        #expect(gain.read("18") == 18)
        #expect(gain.read("40") == 24)
        #expect(BandAxis.frequency(phoneLimits).read("5") == 20)
    }

    @Test func textThatIsNoNumberIsRefused() {
        #expect(BandAxis.gain(around: 0, limits: phoneLimits).read("loud") == nil)
        #expect(BandAxis.frequency(phoneLimits).read("") == nil)
        #expect(BandAxis.width(phoneLimits).read("nan") == nil)
    }
}

@Suite struct BandNudgeTests {
    @Test func aNudgeStepsTheGainOfABandThatHasOne() {
        #expect(BandNudge.level(band(gain: 2), up: false, limits: phoneLimits).gainDb == 1.5)
    }

    @Test func aNudgeStepsTheQOfABandWithoutGain() {
        let nudged = BandNudge.level(band(.notch, q: 1), up: true, limits: phoneLimits)

        #expect(nudged.q == 1.12)
        #expect(nudged.gainDb == 0)
    }

    @Test func aFrequencyNudgeIsAThirdOfAnOctave() {
        #expect(BandNudge.frequency(band(hertz: 1_000), up: false, limits: phoneLimits).frequencyHz == 794)
        #expect(BandNudge.frequency(band(hertz: 1_000), up: true, limits: phoneLimits).frequencyHz == 1_260)
        #expect(BandNudge.frequency(band(hertz: 19_000), up: true, limits: phoneLimits).frequencyHz == 20_000)
    }

    @Test func aFilterWithoutGainSaysSoWhereTheGainWouldBe() {
        #expect(EqualiserFilter.notch.gainUnused == "Not used by notch")
        #expect(EqualiserFilter.lowPass.gainUnused == "Not used by low pass")
    }
}

@Suite struct EqualiserDraftTests {
    let committed = curve([band(hertz: 1_000)])
    let moving = curve([band(hertz: 2_000)])

    @Test func aHeldDraftIsShownOverTheCommittedCurve() {
        var draft = EqualiserDraft()
        draft.hold(moving)

        #expect(draft.shown(over: committed) == moving)
        #expect(draft.holding)
    }

    @Test func aCurveCommittedWhileAHandleIsHeldLeavesTheDraftShown() {
        var draft = EqualiserDraft()
        draft.hold(moving)

        draft.arrived(committed)

        #expect(draft.shown(over: committed) == moving)
    }

    @Test func theReleasedCurveArrivingGivesTheDraftUp() {
        var draft = EqualiserDraft()
        draft.hold(moving)
        draft.release(moving)

        draft.arrived(moving)

        #expect(draft.curve == nil)
        #expect(!draft.holding)
    }

    @Test func anyOtherCurveArrivingAfterTheReleaseAlsoGivesItUp() {
        var draft = EqualiserDraft()
        draft.release(moving)

        draft.arrived(committed)

        #expect(draft.curve == nil)
    }

    @Test func aReleaseNoAnswerFollowsIsDroppedOnceItSettles() {
        var draft = EqualiserDraft()
        draft.release(moving)

        draft.settle(moving)

        #expect(draft.curve == nil)
    }

    @Test func settlingAnOlderReleaseLeavesANewerHoldAlone() {
        var draft = EqualiserDraft()
        draft.release(moving)
        draft.hold(committed)

        draft.settle(moving)

        #expect(draft.curve == committed)
    }

    @Test func aDragTheSystemCancelsIsDropped() {
        var draft = EqualiserDraft()
        draft.hold(moving)

        draft.drop()

        #expect(draft.shown(over: committed) == committed)
        #expect(!draft.holding)
    }
}

@Suite struct TellingPaceTests {
    @Test func theFirstMoveIsToldAtOnceAndOnesSoonAfterWait() {
        var pace = TellingPace(every: .milliseconds(50))
        let start = ContinuousClock.now

        #expect(pace.ask(at: start) == nil)
        #expect(pace.ask(at: start + .milliseconds(20)) == .milliseconds(30))
        #expect(pace.ask(at: start + .milliseconds(50)) == nil)
    }

    @Test func aMoveToldLateCountsFromWhenItWasTold() {
        var pace = TellingPace(every: .milliseconds(50))
        let start = ContinuousClock.now
        _ = pace.ask(at: start)

        pace.told(at: start + .milliseconds(40))

        #expect(pace.ask(at: start + .milliseconds(60)) == .milliseconds(30))
    }

    @Test func aPaceForgottenTellsTheNextMoveAtOnce() {
        var pace = TellingPace(every: .milliseconds(50))
        let start = ContinuousClock.now
        _ = pace.ask(at: start)

        pace.forget()

        #expect(pace.ask(at: start + .milliseconds(1)) == nil)
    }
}

@Suite struct HeadphoneMatchesTests {
    @Test func atMostTwentyAreShownAndTheRestCounted() {
        let found = HeadphoneMatches.shown(Array(0..<57))

        #expect(found.shown.count == 20)
        #expect(found.more == 37)
        #expect(HeadphoneMatches.shown([1, 2]).more == 0)
    }
}
