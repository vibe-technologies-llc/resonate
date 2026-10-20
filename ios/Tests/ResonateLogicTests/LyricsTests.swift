import Foundation
import ResonateNative
import Testing

@testable import ResonateLogic

struct DesktopLyrics {
    let source: String

    init() throws {
        let repository = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        source = try String(contentsOf: repository.appendingPathComponent("crates/resonate-ui/src/lyrics.rs"), encoding: .utf8)
    }

    func seconds(_ name: String) throws -> Double {
        let pattern = try NSRegularExpression(pattern: #"^const \#(name): Duration = Duration::from_(millis|secs)\(([0-9_]+)\);"#, options: .anchorsMatchLines)
        let range = NSRange(source.startIndex..., in: source)
        let found = try #require(pattern.firstMatch(in: source, range: range), "the desktop has no \(name)")
        let unit = String(source[Range(found.range(at: 1), in: source)!])
        let count = try #require(Double(source[Range(found.range(at: 2), in: source)!].replacingOccurrences(of: "_", with: "")))
        return unit == "millis" ? count / 1000 : count
    }

    func number(_ name: String) throws -> Double {
        let pattern = try NSRegularExpression(pattern: #"^const \#(name): (?:f32|usize) = ([0-9_.]+);"#, options: .anchorsMatchLines)
        let range = NSRange(source.startIndex..., in: source)
        let found = try #require(pattern.firstMatch(in: source, range: range), "the desktop has no \(name)")
        return try #require(Double(source[Range(found.range(at: 1), in: source)!].replacingOccurrences(of: "_", with: "")))
    }
}

@Suite struct LyricPaceTests {
    @Test func theTimingsTheSwiftPaneSharesWithTheDesktopPaneAreTheDesktops() throws {
        let desktop = try DesktopLyrics()

        #expect(try desktop.seconds("TURN") == LyricPace.turn)
        #expect(try desktop.seconds("GLIDE") == LyricPace.glide)
        #expect(try desktop.seconds("LAG_PER_LINE") == LyricPace.lagPerLine)
        #expect(try desktop.seconds("ARRIVES_IN") == LyricPace.arrivesIn)
        #expect(try desktop.seconds("RISE") == LyricPace.rise)
        #expect(try desktop.seconds("RISE_PER_LINE") == LyricPace.risePerLine)
        #expect(try desktop.seconds("LOOKS_QUIETLY_FOR") == 0.45)
        #expect(try desktop.seconds("BREATH") == LyricPace.breath)
        #expect(try desktop.seconds("DRIFTS_AT_MOST") == LyricPace.driftsAtMost)
        #expect(try desktop.seconds("STEERED_OVER") == LyricPace.steeredOver)
        #expect(try desktop.seconds("HANDS_OFF") == LyricPace.handsOff)
    }

    @Test func theCountsTheSwiftPaneSharesWithTheDesktopPaneAreTheDesktops() throws {
        let desktop = try DesktopLyrics()

        #expect(try desktop.number("LAUNCHED_AT_MOST") == LyricPace.launchedAtMost)
        #expect(try desktop.number("LAGS_AT_MOST") == Double(LyricPace.lagsAtMost))
        #expect(try desktop.number("RISES_AT_MOST") == Double(LyricPace.risesAtMost))
        #expect(try desktop.number("RISES_FROM") == Double(LyricPace.rise22))
        #expect(try desktop.number("DRAWN_WITHIN_PANES") == Double(LyricPace.drawsBeyondPanes))
    }

    @Test func aGlideLandsAndLaunchesAtRestAtBothEnds() {
        #expect(abs(LyricPace.landed(0)) < 1e-12)
        #expect(abs(LyricPace.landed(1) - 1) < 1e-12)
        #expect(abs(LyricPace.landingPace(0)) < 1e-12)
        #expect(abs(LyricPace.landingPace(1)) < 1e-12)
        #expect(abs(LyricPace.launched(0)) < 1e-12)
        #expect(abs(LyricPace.launched(1)) < 1e-12)
        #expect(abs(LyricPace.launchPace(0) - 1) < 1e-12)
    }

    @Test func aLandingNeverPassesItsEndAndNeverTurnsBack() {
        var was = 0.0
        for step in 1...1_000 {
            let through = LyricPace.landed(Double(step) / 1_000)
            #expect(through >= was && through <= 1)
            was = through
        }
    }

    @Test func theEasingRunsFromNothingToEverythingThroughTheMiddle() {
        #expect(LyricPace.easedInOut(0) == 0)
        #expect(LyricPace.easedInOut(0.5) == 0.5)
        #expect(LyricPace.easedInOut(1) == 1)
        #expect(LyricPace.easedInOut(0.25) < 0.25)
        #expect(LyricPace.easedInOut(0.75) > 0.75)
    }

    @Test func aShareOfASpanIsHeldBetweenNothingAndAll() {
        #expect(LyricPace.share(-1, of: 2) == 0)
        #expect(LyricPace.share(1, of: 2) == 0.5)
        #expect(LyricPace.share(9, of: 2) == 1)
    }

    @Test func linesLagByTheirCountHeldToTheLimit() {
        #expect(LyricPace.lagged(0, by: 0.014, atMost: 6) == 0)
        #expect(abs(LyricPace.lagged(3, by: 0.014, atMost: 6) - 0.042) < 1e-12)
        #expect(abs(LyricPace.lagged(60, by: 0.014, atMost: 6) - 0.084) < 1e-12)
        #expect(LyricPace.lagged(-3, by: 0.014, atMost: 6) == 0)
    }
}

extension LyricPace {
    static let rise22 = 22
}

@Suite struct LyricFalloffTests {
    @Test func theLitLineIsFullAndTheOnesAfterItFallAwayAsTheDesktops() {
        #expect(LyricFalloff.around.ahead(0) == 1)
        #expect(LyricFalloff.around.ahead(1) == 0.4)
        #expect(LyricFalloff.around.ahead(2) == 0.18)
        #expect(LyricFalloff.around.ahead(3) == 0)
        #expect(LyricFalloff.across.ahead(1) == 0.42)
        #expect(LyricFalloff.across.ahead(2) == 0.3)
        #expect(LyricFalloff.across.ahead(3) == 0.22)
        #expect(LyricFalloff.across.ahead(9) == 0.16)
    }

    @Test func aLineSungIsOutOfAnInPlayReadingAndStillInTheWholeSet() {
        #expect(LyricFalloff.around.between(sung: 4, line: 2) == 0)
        #expect(LyricFalloff.across.between(sung: 4, line: 2) == LyricFalloff.across.ahead(2))
        #expect(LyricFalloff.across.between(sung: 4, line: 4) == 1)
    }

    @Test func aWaitLeavesTheLineWaitedOnAndTheOneAfterReadableUnderTheDots() {
        #expect(LyricFalloff.around.spent(read: 5, line: 5) == LyricFalloff.around.ahead(1))
        #expect(LyricFalloff.around.spent(read: 5, line: 6) == LyricFalloff.around.ahead(2))
        #expect(LyricFalloff.around.spent(read: 5, line: 4) == 0)
        #expect(LyricFalloff.around.spent(read: nil, line: 0) == 0)
        #expect(LyricFalloff.across.spent(read: 5, line: 5) == LyricFalloff.across.ahead(1))
        #expect(LyricFalloff.across.spent(read: nil, line: 0) == LyricFalloff.across.ahead(.max))
    }
}

@Suite struct LyricTurnTests {
    @Test func aTurnRunsFromNothingToEverythingAndThenStaysThere() {
        var turn = LyricTurn(still: 0.0)
        turn.onto(1.0, at: 100)

        #expect(turn.blended(at: 100) { $0 } == 0)
        #expect(abs(turn.blended(at: 100 + LyricPace.turn / 2) { $0 } - 0.5) < 1e-12)
        #expect(turn.blended(at: 100 + LyricPace.turn) { $0 } == 1)
        #expect(turn.blended(at: 100 + LyricPace.turn * 4) { $0 } == 1)
        #expect(!turn.settled(at: 100))
        #expect(turn.settled(at: 100 + LyricPace.turn))
    }

    @Test func aTurnStillIsSettledAndShowsItsValue() {
        let turn = LyricTurn(still: 3.0)

        #expect(turn.settled(at: 0))
        #expect(turn.blended(at: 0) { $0 } == 3)
    }

    @Test func turningOntoTheValueItAlreadyHasChangesNothing() {
        var turn = LyricTurn(still: 1.0)

        turn.onto(1.0, at: 50)

        #expect(turn.settled(at: 50))
    }

    @Test func aLineCrossfadesFromWhereItStoodToWhereItIsGoing() {
        var turn = LyricTurn(still: LyricFalloff.across.ahead(1))
        turn.onto(LyricPace.lit, at: 10)

        #expect(abs(turn.blended(at: 10) { $0 } - 0.42) < 1e-12)
        #expect(turn.blended(at: 10 + LyricPace.turn) { $0 } == 1)
    }
}

@Suite struct LyricGlideTests {
    let frame: TimeInterval = 4.167e-3

    func frames(through span: TimeInterval) -> [TimeInterval] {
        let count = Int(span / frame) + 2
        return (0...count).map { Double($0) * frame }
    }

    @Test func aGlideRunsFromRestToItsLandingAndNeverPassesIt() {
        for travel in [22.0, 66, 120, 400, -900, 2_400] {
            let glide = LyricGlide.fromRest(was: 0, lands: travel, at: 0)
            var drawn = 0.0

            for elapsed in frames(through: LyricPace.glide) {
                let now = glide.at(elapsed)
                #expect(abs(travel - now) <= abs(travel - drawn) + 1e-9, "a glide of \(travel) turned back at \(elapsed)")
                drawn = now
            }
            #expect(abs(drawn - travel) < 1e-6, "a glide of \(travel) did not land")
            #expect(glide.settled(at: LyricPace.glide + LyricPace.lagPerLine * 6))
        }
    }

    @Test func aGlideNotYetOutIsNotSettled() {
        let glide = LyricGlide.fromRest(was: 0, lands: 100, at: 0)

        #expect(!glide.settled(at: LyricPace.glide))
        #expect(!glide.settled(at: 0))
    }

    @Test func aNewLandingInFlightCarriesTheGlideOnWithoutAJumpOrAStop() {
        let first = LyricGlide.fromRest(was: 0, lands: -400, at: 0)
        let turned = LyricPace.glide / 3
        let then = first.redirected(to: -466, at: turned)

        #expect(abs(then.at(turned) - first.at(turned)) < 1e-3)
        for lines in 0...LyricPace.lagsAtMost {
            let lag = LyricPace.lagged(lines, by: LyricPace.lagPerLine, atMost: LyricPace.lagsAtMost)
            #expect(abs(then.lagging(lag, at: turned) - first.lagging(lag, at: turned)) < 1e-3, "a line \(lines) ahead jumped")
        }

        let before = first.at(turned) - first.at(turned - frame)
        let after = then.at(turned + frame) - then.at(turned)
        #expect(abs(after - before) < abs(before) * 0.1, "the glide went from \(before) to \(after) a frame as it turned")
    }

    @Test func aRedirectToJustBehindWhereTheGlideIsNeverOvershootsItsNewLanding() {
        let first = LyricGlide.fromRest(was: 0, lands: -400, at: 0)
        let turned = LyricPace.glide / 3
        let near = first.redirected(to: first.at(turned) - 3, at: turned)

        for elapsed in frames(through: LyricPace.glide) {
            #expect(near.at(turned + elapsed) >= near.leg.lands - 1e-3)
        }
    }

    @Test func aLineFurtherAheadLagsFurtherBehindTheGlideAndCatchesUpOnceItSettles() {
        let glide = LyricGlide.fromRest(was: 0, lands: -300, at: 0)
        let midway = 0.12

        let near = glide.lagging(LyricPace.lagged(1, by: LyricPace.lagPerLine, atMost: 6), at: midway)
        let far = glide.lagging(LyricPace.lagged(5, by: LyricPace.lagPerLine, atMost: 6), at: midway)

        #expect(far > near)
        #expect(glide.lagging(0.084, at: LyricPace.glide + 0.084) == -300)
    }

    @Test func aLegsPaceIsClampedToTheMostItMayBeLaunchedAt() {
        let leg = LyricLeg.launched(from: 0, to: 10, pace: 10_000, at: 0)

        #expect(leg.launch == 10 * LyricPace.launchedAtMost)
        #expect(LyricLeg.launched(from: 0, to: 10, pace: -10_000, at: 0).launch == -10 * LyricPace.launchedAtMost)
    }

    @Test func aLegHasLandedOnceItsGlideIsOut() {
        let leg = LyricLeg(was: 0, lands: 5, launch: 0, started: 1)

        #expect(!leg.landed(at: 1 + LyricPace.glide - 0.001))
        #expect(leg.landed(at: 1 + LyricPace.glide))
        #expect(leg.elapsed(at: 0) == 0)
    }
}

@Suite struct LyricClockTests {
    func heard(_ seconds: Double, track: UInt64 = 1, playing: Bool = true) -> LyricPlayback {
        LyricPlayback(track: track, positionMs: UInt64(seconds * 1000), playing: playing)
    }

    @Test func theClockRunsSmoothlyThroughAPositionThatArrivesADecodedBlockAtATime() {
        let block = 0.093
        let starts = 10.0
        for frame in [0.016667, 0.004167] {
            var clock = LyricClock()
            let frames = Int(4.0 / frame)

            var was = clock.kept(heard(starts), at: 0)
            #expect(was == starts)
            for step in 1...frames {
                let elapsed = frame * Double(step)
                let sampled = starts + block * (elapsed / block).rounded(.down)
                let kept = clock.kept(heard(sampled), at: elapsed)
                #expect(kept >= was, "the clock ran backwards at frame \(step)")
                #expect(abs((kept - was) - frame) < frame / 4, "a frame of \(frame) moved the clock by \(kept - was)")
                #expect(abs((starts + elapsed) - kept) < block)
                was = kept
            }
        }
    }

    @Test func aClockSteeredBackByAPositionBehindItSlowsRatherThanRunningBackwards() {
        var clock = LyricClock()
        _ = clock.kept(heard(10), at: 0)

        let kept = clock.kept(heard(10 - 0.04), at: 0.2)

        #expect(kept >= 10)
        #expect(kept < 10.2)
    }

    @Test func aSeekTakesThePositionAsItStands() {
        var clock = LyricClock()
        _ = clock.kept(heard(10), at: 0)

        let sought = clock.kept(heard(90), at: 4)

        #expect(sought == 90)
    }

    @Test func aPauseHoldsThePositionAndRunsNoMoreWhateverTheTimeThatPasses() {
        var clock = LyricClock()
        _ = clock.kept(heard(10), at: 0)

        #expect(clock.kept(heard(91, playing: false), at: 4) == 91)
        #expect(clock.kept(heard(91, playing: false), at: 9) == 91)
    }

    @Test func aNewTrackTakesItsPositionAsItStands() {
        var clock = LyricClock()
        _ = clock.kept(heard(10, track: 1), at: 0)

        #expect(clock.kept(heard(0.1, track: 2), at: 0.2) == 0.1 + 0)
    }

    @Test func aPositionFarPastTheDriftAllowedIsTakenAsItStands() {
        var clock = LyricClock()
        _ = clock.kept(heard(10), at: 0)

        #expect(clock.kept(heard(11), at: 0.1) == 11 + 0)
    }

    @Test func stoppingForgetsEverything() {
        var clock = LyricClock()
        _ = clock.kept(heard(10), at: 0)

        clock.stop()

        #expect(clock.kept(heard(50), at: 1) == 50)
    }
}

@Suite struct LyricHeardTests {
    @Test func aSnapshotHearsAPlayingSongOnlyWhereItIsTheOneBeingShown() {
        let playing = snapshot(.playing, current: track(7))

        #expect(LyricHeard.of(playing, showing: track(7)) == LyricHeard(track: 7, playing: true))
        #expect(LyricHeard.of(snapshot(.paused, current: track(7)), showing: track(7)) == LyricHeard(track: 7, playing: false))
        #expect(LyricHeard.of(playing, showing: track(8)) == nil)
        #expect(LyricHeard.of(playing, showing: nil) == nil)
        #expect(LyricHeard.of(nil, showing: track(7)) == nil)
        #expect(LyricHeard.of(snapshot(.playing), showing: track(7)) == nil)
    }
}

@Suite struct LyricBreathingTests {
    @Test func aBreathFadesInOverATurnAndTheOldOneOutOverTheSame() {
        let breathing = LyricBreathing(line: 3, started: 0)

        #expect(breathing.opacity(at: 0) == 0)
        #expect(breathing.opacity(at: LyricPace.turn) == 1)
        #expect(breathing.fading(at: 0) == 1)
        #expect(breathing.fading(at: LyricPace.turn) == 0)
        #expect(!breathing.settled(at: 0.2))
        #expect(breathing.settled(at: LyricPace.turn))
    }
}
