import Foundation
import Testing

@testable import ResonateLogic

@Suite struct MotionTests {
    private static func cubicOut(_ time: Double) -> Double { 1 - pow(1 - time, 3) }
    private static func smoothstep(_ time: Double) -> Double { time * time * (3 - 2 * time) }

    private static let playingFree = ProgressFlow.Conditions(
        playing: true,
        touched: false,
        held: nil,
        reduceMotion: false,
        songChanged: false
    )

    @Test func theSettlingCurveIsTheDesktopsCubicEaseOut() {
        for time in stride(from: 0.0, through: 1.0, by: 0.05) {
            #expect(abs(MotionCurve.settling.progress(at: time) - Self.cubicOut(time)) < 1e-6)
        }
    }

    @Test func theSmoothCurveIsTheDesktopsSmoothstep() {
        for time in stride(from: 0.0, through: 1.0, by: 0.05) {
            #expect(abs(MotionCurve.smooth.progress(at: time) - Self.smoothstep(time)) < 1e-6)
        }
    }

    @Test func theSpansAreTheDesktops() {
        #expect(MotionSpan.arrivesOver == 0.15)
        #expect(MotionSpan.flipsOver == 0.14)
        #expect(MotionSpan.handsOver == 0.28)
    }

    @Test func aPauseOrAnyRedrawOfTheSameSongIsNotASongChange() {
        #expect(SongChange(from: "7", to: "7") == .unchanged)
        #expect(SongChange(from: nil, to: nil) == .unchanged)
        #expect(!SongChange(from: "7", to: "7").handsOver)
    }

    @Test func aSkipIsAHandoverWhicheverWayItGoes() {
        #expect(SongChange(from: "7", to: "8") == .handed)
        #expect(SongChange(from: "8", to: "7") == .handed)
        #expect(SongChange(from: "7", to: "8").handsOver)
    }

    @Test func aSongAppearingOrLeavingIsAChangeToo() {
        #expect(SongChange(from: nil, to: "7") == .arrived)
        #expect(SongChange(from: "7", to: nil) == .cleared)
        #expect(SongChange(from: nil, to: "7").handsOver)
        #expect(SongChange(from: "7", to: nil).handsOver)
    }

    @Test func aServedSongIsKeyedByItsAddress() {
        #expect(SongChange(from: "subsonic:track/1", to: "subsonic:track/2") == .handed)
        #expect(SongChange(from: "subsonic:track/1", to: "subsonic:track/1") == .unchanged)
    }

    @Test func nothingMovesWhileTheSceneIsAway() {
        for kind in [MotionKind.handover, .arrival, .flip, .progress] {
            #expect(MotionRendering.of(kind, reduceMotion: false, sceneActive: false) == .instant)
            #expect(MotionRendering.of(kind, reduceMotion: true, sceneActive: false) == .instant)
        }
    }

    @Test func everythingMovesWhereMotionIsWelcome() {
        for kind in [MotionKind.handover, .arrival, .flip, .progress] {
            #expect(MotionRendering.of(kind, reduceMotion: false, sceneActive: true) == .animated)
        }
    }

    @Test func reduceMotionKeepsOnlyTheFadeOfAnArrival() {
        #expect(MotionRendering.of(.arrival, reduceMotion: true, sceneActive: true) == .fades)
        #expect(MotionRendering.of(.handover, reduceMotion: true, sceneActive: true) == .instant)
        #expect(MotionRendering.of(.flip, reduceMotion: true, sceneActive: true) == .instant)
        #expect(MotionRendering.of(.progress, reduceMotion: true, sceneActive: true) == .instant)
    }

    @Test func aPlayingReadingFlowsToWhereTheSongWillBeTwoReadingsLater() {
        let step = ProgressFlow.step(.init(old: 10_000, now: 10_250, sinceLast: 0.25), Self.playingFree)

        #expect(step == .flow(to: 10_750, over: 0.5))
    }

    @Test func aFlowMovesAtTheSongsOwnPaceWhateverItsHorizon() {
        let step = ProgressFlow.step(.init(old: 10_000, now: 10_250, sinceLast: 0.25), Self.playingFree)

        guard case .flow(let target, let over) = step else {
            Issue.record("a playing reading should flow")
            return
        }
        #expect(abs((target - 10_250) / over - 1000) < 1e-9)
    }

    @Test func aFlowIsHeldWithinTheShortestAndLongestWait() {
        let quick = ProgressFlow.step(.init(old: 0, now: 10, sinceLast: 0.001), Self.playingFree)
        let slow = ProgressFlow.step(.init(old: 0, now: 10, sinceLast: 30), Self.playingFree)

        #expect(quick == .flow(to: 110, over: 0.1))
        #expect(slow == .flow(to: 1210, over: 1.2))
    }

    @Test func theFirstReadingAfterOpeningSetsTheBarAtOnce() {
        let step = ProgressFlow.step(.init(old: 5_000, now: 5_000, sinceLast: nil), Self.playingFree)

        #expect(step == .snap(to: 5_000))
    }

    @Test func aSeekBackOrAcrossASongNeverAnimatesTheBar() {
        let back = ProgressFlow.step(.init(old: 90_000, now: 12_000, sinceLast: 0.25), Self.playingFree)
        let across = ProgressFlow.step(.init(old: 12_000, now: 90_000, sinceLast: 0.25), Self.playingFree)

        #expect(back == .snap(to: 12_000))
        #expect(across == .snap(to: 90_000))
    }

    @Test func aNewSongSetsTheBarAtOnceEvenWhereTheClockOnlyMovedForward() {
        let conditions = ProgressFlow.Conditions(playing: true, touched: false, held: nil, reduceMotion: false, songChanged: true)

        let step = ProgressFlow.step(.init(old: 0, now: 200, sinceLast: 0.25), conditions)

        #expect(step == .snap(to: 200))
    }

    @Test func aPausedOrTouchedBarDoesNotFlow() {
        let paused = ProgressFlow.Conditions(playing: false, touched: false, held: nil, reduceMotion: false, songChanged: false)
        let touched = ProgressFlow.Conditions(playing: true, touched: true, held: nil, reduceMotion: false, songChanged: false)

        #expect(ProgressFlow.step(.init(old: 0, now: 250, sinceLast: 0.25), paused) == .snap(to: 250))
        #expect(ProgressFlow.step(.init(old: 0, now: 250, sinceLast: 0.25), touched) == .snap(to: 250))
    }

    @Test func aHeldSeekStaysWhereItWasSent() {
        let conditions = ProgressFlow.Conditions(playing: true, touched: false, held: 120_000, reduceMotion: false, songChanged: false)

        #expect(ProgressFlow.step(.init(old: 0, now: 250, sinceLast: 0.25), conditions) == .snap(to: 120_000))
    }

    @Test func reduceMotionSetsTheBarAtOnce() {
        let conditions = ProgressFlow.Conditions(playing: true, touched: false, held: nil, reduceMotion: true, songChanged: false)

        #expect(ProgressFlow.step(.init(old: 0, now: 250, sinceLast: 0.25), conditions) == .snap(to: 250))
    }

    @Test func theTitlesKeepTheTallestHeightSeenAtOneWidth() {
        var held = HeldHeight()

        held.see(width: 300, height: 90)
        held.see(width: 300, height: 60)
        held.see(width: 300, height: 120)
        held.see(width: 300, height: 70)

        #expect(held.height == 120)
    }

    @Test func aNewWidthStartsTheHeightAgain() {
        var held = HeldHeight()

        held.see(width: 300, height: 120)
        held.see(width: 620, height: 70)

        #expect(held.width == 620)
        #expect(held.height == 70)
    }

    @Test func aWidthThatMovedByAHairlineIsTheSameWidth() {
        var held = HeldHeight()

        held.see(width: 300, height: 120)
        held.see(width: 300.2, height: 60)

        #expect(held.height == 120)
    }

    @Test func aSwipeMostlyDownAndFarEnoughClosesTheCover() {
        #expect(CoverSwipe.closes(across: 20, down: 120, flungDown: 260))
    }

    @Test func aSwipeTooShortOrMostlyAcrossLeavesTheCoverOpen() {
        #expect(!CoverSwipe.closes(across: 0, down: 40, flungDown: 200))
        #expect(!CoverSwipe.closes(across: 150, down: 120, flungDown: 260))
        #expect(!CoverSwipe.closes(across: 0, down: -120, flungDown: -260))
    }

    @Test func aSwipeDrawnBackUpBeforeLettingGoLeavesTheCoverOpen() {
        #expect(!CoverSwipe.closes(across: 0, down: 100, flungDown: 40))
    }

    @Test func aSliderDragIsUndecidedUntilItMovesPastTheThreshold() {
        #expect(SliderDrag.undecided.after(across: 3, down: 5) == .undecided)
    }

    @Test func aDragMostlyDownIsLeftToTheCoversDismissal() {
        #expect(SliderDrag.undecided.after(across: 4, down: 12) == .declined)
        #expect(SliderDrag.undecided.after(across: -2, down: -9) == .declined)
    }

    @Test func aDragMostlyAcrossSlides() {
        #expect(SliderDrag.undecided.after(across: -12, down: 4) == .sliding)
    }

    @Test func aDecidedDragKeepsItsHeadingWhereverItWandersAfter() {
        #expect(SliderDrag.sliding.after(across: 0, down: 80) == .sliding)
        #expect(SliderDrag.declined.after(across: 80, down: 0) == .declined)
    }
}
