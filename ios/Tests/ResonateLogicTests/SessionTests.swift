import Foundation
import ResonateNative
import Testing

@testable import ResonateLogic

@Suite struct InterruptionPolicyTests {
    @Test func aCallBeginningPausesEveryZoneThatSoundsAndTheEndResumesThoseItPaused() {
        var policy = InterruptionPolicy()

        let began = policy.heard(.began)
        policy.paused([1, 3])
        let owed = policy.resumesAfterwards
        let ended = policy.heard(.ended(shouldResume: true))

        #expect(began == .pauseSounding)
        #expect(owed == [1, 3])
        #expect(ended == .resume([1, 3]))
        #expect(policy.resumesAfterwards.isEmpty)
    }

    @Test func anInterruptionThatFoundNothingSoundingNeverStartsMusicWhenItEnds() {
        var policy = InterruptionPolicy()

        let began = policy.heard(.began)
        policy.paused([])
        let ended = policy.heard(.ended(shouldResume: true))

        #expect(began == .pauseSounding)
        #expect(ended == .nothing)
    }

    @Test func anInterruptionEndingWithoutTheSystemsLeaveDoesNotResume() {
        var policy = InterruptionPolicy()
        _ = policy.heard(.began)
        policy.paused([2])

        let ended = policy.heard(.ended(shouldResume: false))

        #expect(ended == .nothing)
        #expect(policy.resumesAfterwards.isEmpty)
    }

    @Test func anEndWithNoBeginningDoesNothingAndNeverResumesTwice() {
        var policy = InterruptionPolicy()

        let unbegun = policy.heard(.ended(shouldResume: true))
        _ = policy.heard(.began)
        policy.paused([1])
        let first = policy.heard(.ended(shouldResume: true))
        let second = policy.heard(.ended(shouldResume: true))

        #expect(unbegun == .nothing)
        #expect(first == .resume([1]))
        #expect(second == .nothing)
    }

    @Test func zonesPausedOutsideAnInterruptionAreNeverResumed() {
        var policy = InterruptionPolicy()

        policy.paused([1, 2])
        _ = policy.heard(.began)
        let ended = policy.heard(.ended(shouldResume: true))

        #expect(ended == .nothing)
    }

    @Test func aSecondBeginningKeepsTheZonesTheFirstPausedOnce() {
        var policy = InterruptionPolicy()
        _ = policy.heard(.began)
        policy.paused([2])

        let again = policy.heard(.began)
        policy.paused([])
        policy.paused([2, 1])
        let ended = policy.heard(.ended(shouldResume: true))

        #expect(again == .pauseSounding)
        #expect(ended == .resume([2, 1]))
    }

    @Test func aTypeTheSystemSentWithoutOneIsIgnored() {
        var policy = InterruptionPolicy()
        _ = policy.heard(.began)
        policy.paused([1])

        let unnamed = policy.heard(nil)

        #expect(unnamed == .nothing)
        #expect(policy.resumesAfterwards == [1])
    }

    @Test func theHeadphonesLeavingPausesEveryZoneThatSoundsAndTakesBackAnyOwedResume() {
        var policy = InterruptionPolicy()
        _ = policy.heard(.began)
        policy.paused([1, 2])

        let lost = policy.lostTheDevice()
        policy.paused([3])
        let ended = policy.heard(.ended(shouldResume: true))

        #expect(lost == .pauseSounding)
        #expect(ended == .nothing)
    }
}

@Suite struct SessionReleaseTests {
    @Test func stoppingAfterPlayingOwesTheRelease() {
        var release = SessionRelease()

        let outcome1 = release.owed(by: snapshot(.playing), listening: false)
        #expect(!outcome1)
        let outcome2 = release.owed(by: snapshot(.stopped), listening: false)
        #expect(outcome2)
        let outcome3 = release.owed(by: snapshot(.stopped), listening: false)
        #expect(!outcome3)
    }

    @Test func aFirstSnapshotThatIsStoppedOwesNothing() {
        var release = SessionRelease()

        let outcome1 = release.owed(by: snapshot(.stopped), listening: false)
        #expect(!outcome1)
    }

    @Test func aSleepTimerPausingTheMusicOwesTheRelease() {
        var release = SessionRelease()

        let outcome1 = release.owed(by: snapshot(.playing, sleepRemainingMs: 5_000), listening: false)
        #expect(!outcome1)
        let outcome2 = release.owed(by: snapshot(.paused), listening: false)
        #expect(outcome2)
    }

    @Test func aPauseWithNoTimerRunningKeepsTheSession() {
        var release = SessionRelease()

        let outcome1 = release.owed(by: snapshot(.playing), listening: false)
        #expect(!outcome1)
        let outcome2 = release.owed(by: snapshot(.paused), listening: false)
        #expect(!outcome2)
    }

    @Test func aPauseWhileTheTimerStillRunsKeepsTheSession() {
        var release = SessionRelease()

        let outcome1 = release.owed(by: snapshot(.playing, sleepRemainingMs: 5_000), listening: false)
        #expect(!outcome1)
        let outcome2 = release.owed(by: snapshot(.paused, sleepRemainingMs: 4_000), listening: false)
        #expect(!outcome2)
    }

    @Test func theZoneShownStoppingKeepsTheSessionWhileAnotherZoneSounds() {
        var release = SessionRelease()

        let playing = release.owed(by: snapshot(.playing, heardElsewhere: soundingElsewhere()), listening: false)
        let stoppedWhileHeard = release.owed(by: snapshot(.stopped, heardElsewhere: soundingElsewhere()), listening: false)
        let lastFellSilent = release.owed(by: snapshot(.stopped), listening: false)

        #expect(!playing)
        #expect(!stoppedWhileHeard)
        #expect(lastFellSilent)
    }

    @Test func aSleepTimerPausingTheZoneShownKeepsTheSessionWhileAnotherZoneSounds() {
        var release = SessionRelease()

        _ = release.owed(by: snapshot(.playing, sleepRemainingMs: 5_000, heardElsewhere: soundingElsewhere()), listening: false)
        let sleptOff = release.owed(by: snapshot(.paused, heardElsewhere: soundingElsewhere()), listening: false)

        #expect(!sleptOff)
    }

    @Test func showingAStoppedZoneIsNotAStopThoughTheOneLeftWasPlaying() {
        var release = SessionRelease()

        _ = release.owed(by: snapshot(.playing, zone: 1), listening: false)
        let shownAnother = release.owed(by: snapshot(.stopped, zone: 2, heardElsewhere: soundingElsewhere(in: 1)), listening: false)
        let thatOneStopped = release.owed(by: snapshot(.stopped, zone: 2), listening: false)

        #expect(!shownAnother)
        #expect(thatOneStopped)
    }

    @Test func showingAStoppedZoneWhileTheOneLeftIsPausedOwesNothing() {
        var release = SessionRelease()

        _ = release.owed(by: snapshot(.paused, zone: 1), listening: false)
        let shownAnother = release.owed(by: snapshot(.stopped, zone: 2), listening: false)

        #expect(!shownAnother)
    }

    @Test func aZoneHeardElsewhereFallingSilentKeepsTheSessionWhileTheZoneShownIsPaused() {
        var release = SessionRelease()

        _ = release.owed(by: snapshot(.paused, heardElsewhere: soundingElsewhere()), listening: false)
        let elsewhereSilent = release.owed(by: snapshot(.paused), listening: false)

        #expect(!elsewhereSilent)
    }

    @Test func listeningKeepsTheSessionWhateverStops() {
        var release = SessionRelease()

        _ = release.owed(by: snapshot(.playing), listening: true)

        let outcome1 = release.owed(by: snapshot(.stopped), listening: true)
        #expect(!outcome1)
    }
}

@Suite struct CommandAvailabilityTests {
    func shown(index: Int, count: Int, repeatMode: UInt32 = 0) -> NowPlayingShown {
        NowPlayingShown(
            track: track(1),
            transport: .playing,
            durationMs: 1_000,
            queueIndex: index,
            queueCount: count,
            shuffle: false,
            repeatMode: repeatMode,
            artRevision: 0,
            artworkReady: true
        )
    }

    @Test func nothingLoadedOffersNothing() {
        let none = CommandAvailability(nil)

        #expect(!none.loaded)
        #expect(!none.next)
    }

    @Test func nextIsOfferedBeforeTheLastRow() {
        #expect(CommandAvailability(shown(index: 0, count: 3)).next)
        #expect(CommandAvailability(shown(index: 1, count: 3)).next)
    }

    @Test func nextIsNotOfferedOnTheLastRowUnlessTheQueueRepeats() {
        #expect(!CommandAvailability(shown(index: 2, count: 3)).next)
        #expect(!CommandAvailability(shown(index: 2, count: 3, repeatMode: 1)).next)
        #expect(CommandAvailability(shown(index: 2, count: 3, repeatMode: 2)).next)
    }

    @Test func aQueueNotYetReadCountsAsEndedUnlessItRepeats() {
        #expect(!CommandAvailability(shown(index: 0, count: 0)).next)
    }

    @Test func aZoneHeardElsewhereOffersPlayAndPauseAlone() {
        let heard = NowPlayingShown(
            track: track(9),
            transport: .playing,
            durationMs: 1_000,
            queueIndex: 0,
            queueCount: 3,
            shuffle: false,
            repeatMode: NowPlayingShown.repeatsAll,
            artRevision: 0,
            artworkReady: true,
            zoneHeard: 2
        )

        let available = CommandAvailability(heard)

        #expect(available.loaded)
        #expect(!available.steers)
        #expect(!available.next)
        #expect(CommandAvailability(shown(index: 0, count: 3)).steers)
    }
}

@Suite struct NowPlayingRewriteTests {
    func shown(
        _ id: UInt64 = 1,
        favourite: Bool = false,
        transport: Transport = .playing,
        index: Int = 0,
        count: Int = 3,
        shuffle: Bool = false,
        repeatMode: UInt32 = 0,
        artwork: Bool = true,
        zoneHeard: UInt32? = nil
    ) -> NowPlayingShown {
        NowPlayingShown(
            track: track(id, favourite: favourite),
            transport: transport,
            durationMs: 1_000,
            queueIndex: index,
            queueCount: count,
            shuffle: shuffle,
            repeatMode: repeatMode,
            artRevision: 0,
            artworkReady: artwork,
            zoneHeard: zoneHeard
        )
    }

    @Test func nothingShownYetIsWritten() {
        #expect(shown().rewrites(over: nil, forced: false, drift: .infinity))
    }

    @Test func theSameThingWithNoDriftIsNotWrittenAgain() {
        #expect(!shown().rewrites(over: shown(), forced: false, drift: 100))
    }

    @Test func aDriftPastTheLimitIsAWrittenSeekButTheLimitItselfIsNot() {
        #expect(shown().rewrites(over: shown(), forced: false, drift: 1_501))
        #expect(!shown().rewrites(over: shown(), forced: false, drift: 1_500))
    }

    @Test func aForcedWriteIsMadeWhateverChanged() {
        #expect(shown().rewrites(over: shown(), forced: true, drift: 0))
    }

    @Test(arguments: [
        shownChange { $0.shown(2) },
        shownChange { $0.shown(favourite: true) },
        shownChange { $0.shown(transport: .paused) },
        shownChange { $0.shown(index: 1) },
        shownChange { $0.shown(count: 4) },
        shownChange { $0.shown(shuffle: true) },
        shownChange { $0.shown(repeatMode: 1) },
        shownChange { $0.shown(artwork: false) },
        shownChange { $0.shown(zoneHeard: 2) },
    ])
    func anyChangeOfWhatIsShownIsWritten(change: ShownChange) {
        #expect(change.make(self).rewrites(over: shown(), forced: false, drift: 0))
    }

    @Test func theCommandsAreSetAgainOnlyWhereTheyDepend() {
        #expect(shown().commandsChanged(from: nil))
        #expect(!shown().commandsChanged(from: shown()))
        #expect(!shown(transport: .paused).commandsChanged(from: shown()))
        #expect(!shown(artwork: false).commandsChanged(from: shown()))
        #expect(shown(favourite: true).commandsChanged(from: shown()))
        #expect(shown(index: 1).commandsChanged(from: shown()))
        #expect(shown(count: 9).commandsChanged(from: shown()))
        #expect(shown(shuffle: true).commandsChanged(from: shown()))
        #expect(shown(repeatMode: 2).commandsChanged(from: shown()))
        #expect(shown(zoneHeard: 2).commandsChanged(from: shown()))
    }
}

struct ShownChange: Sendable, CustomTestStringConvertible {
    let make: @Sendable (NowPlayingRewriteTests) -> NowPlayingShown
    var testDescription: String { "a change" }
}

func shownChange(_ make: @escaping @Sendable (NowPlayingRewriteTests) -> NowPlayingShown) -> ShownChange {
    ShownChange(make: make)
}

@Suite struct LandingJournalTests {
    func journal() -> LandingJournal {
        let suite = "landing-\(UUID().uuidString)"
        return LandingJournal(suite: suite)
    }

    @Test func nothingBeganLeavesNothingToRecover() {
        #expect(journal().left == nil)
    }

    @Test func aBeginningIsKeptUntilItIsFinished() {
        let journal = journal()

        journal.begin("resonate-folder://local/a.flac")
        #expect(journal.left == "resonate-folder://local/a.flac")

        journal.finish()
        #expect(journal.left == nil)
    }

    @Test func aLaterBeginningReplacesTheEarlierOne() {
        let journal = journal()
        journal.begin("one")

        journal.begin("two")

        #expect(journal.left == "two")
    }

    @Test func recoveryClearsTheEntryOnlyWhileItIsStillTheOneRecovered() {
        let journal = journal()
        journal.begin("one")

        journal.begin("two")
        journal.finish(ifStill: "one")
        #expect(journal.left == "two")

        journal.finish(ifStill: "two")
        #expect(journal.left == nil)
    }

    @Test func theKeyAndSuiteAreTheOnesTheAppAlwaysKept() {
        #expect(LandingJournal.key == "unfinished-sync-landing")
        #expect(LandingJournal.suite == "library-features")
    }

    @Test func aSuiteIsHeldByNameSoTheJournalIsSendable() {
        let journal: any Sendable = journal()

        #expect(journal is LandingJournal)
    }
}

@Suite struct HoldMachineTests {
    func hold(
        _ machine: inout HoldMachine,
        _ busy: Bool,
        active: Bool = true,
        id: String = "a.1"
    ) -> [HoldEffect] {
        machine.hold(busy, active: active) { id }
    }

    @Test func holdingBeginsAPlainTaskAndAsksToContinueWhileTheAppIsActive() {
        var machine = HoldMachine(continues: true)

        let outcome1 = hold(&machine, true)
        #expect(outcome1 == [.beginPlain, .submit("a.1")])
        #expect(machine.holdingPlain)
    }

    @Test func holdingAgainBeginsNothingMoreAndAsksOnlyOnceWhileAskingStands() {
        var machine = HoldMachine(continues: true)
        let first = hold(&machine, true)
        machine.submitted("a.1", accepted: true)

        #expect(first == [.beginPlain, .submit("a.1")])
        let outcome1 = hold(&machine, true, id: "a.2")
        #expect(outcome1.isEmpty)
    }

    @Test func aHoldThatNeverContinuesBeginsOnlyThePlainTask() {
        var machine = HoldMachine(continues: false)

        let outcome1 = hold(&machine, true)
        #expect(outcome1 == [.beginPlain])
    }

    @Test func aBackgroundedAppBeginsThePlainTaskAndDoesNotAskToContinue() {
        var machine = HoldMachine(continues: true)

        let outcome1 = hold(&machine, true, active: false)
        #expect(outcome1 == [.beginPlain])
    }

    @Test func aRefusedRequestLeavesThePlainTaskAndIsAskedAgainOnTheNextHold() {
        var machine = HoldMachine(continues: true)
        _ = hold(&machine, true)
        machine.submitted("a.1", accepted: false)

        #expect(machine.request == nil)
        let outcome1 = hold(&machine, true, id: "a.2")
        #expect(outcome1 == [.submit("a.2")])
        #expect(machine.holdingPlain)
    }

    @Test func theSystemStartingTheRequestEndsThePlainTaskAndShowsTheProgress() {
        var machine = HoldMachine(continues: true)
        _ = hold(&machine, true)
        machine.submitted("a.1", accepted: true)

        #expect(machine.accepts("a.1"))
        #expect(!machine.accepts("a.9"))
        let outcome1 = machine.began()
        #expect(outcome1 == [.endPlain, .show])
        #expect(machine.continuing)
        #expect(!machine.holdingPlain)
        #expect(machine.request == nil)
    }

    @Test func whileTheSystemContinuesNoNewPlainTaskOrRequestIsMade() {
        var machine = HoldMachine(continues: true)
        _ = hold(&machine, true)
        machine.submitted("a.1", accepted: true)
        _ = machine.began()

        let outcome1 = hold(&machine, true, id: "a.2")
        #expect(outcome1.isEmpty)
    }

    @Test func letGoBeforeTheSystemStartsItEndsThePlainTaskAndForgetsTheRequest() {
        var machine = HoldMachine(continues: true)
        _ = hold(&machine, true)
        machine.submitted("a.1", accepted: true)

        let outcome1 = hold(&machine, false)
        #expect(outcome1 == [.endPlain, .forgetAsked("a.1")])
        #expect(!machine.accepts("a.1"))
        #expect(machine.request == nil)
    }

    @Test func letGoWhileContinuingCompletesTheSystemsTaskSuccessfully() {
        var machine = HoldMachine(continues: true)
        _ = hold(&machine, true)
        machine.submitted("a.1", accepted: true)
        _ = machine.began()

        let outcome1 = hold(&machine, false)
        #expect(outcome1 == [.complete(success: true)])
        #expect(!machine.continuing)
    }

    @Test func expiringTellsTheOwnerThenEndsEverythingUnsuccessfully() {
        var machine = HoldMachine(continues: true)
        _ = hold(&machine, true)
        machine.submitted("a.1", accepted: true)

        let outcome1 = machine.expired()
        #expect(outcome1 == [.notifyExpired, .endPlain, .forgetAsked("a.1")])

        var continuing = HoldMachine(continues: true)
        _ = hold(&continuing, true)
        continuing.submitted("a.1", accepted: true)
        _ = continuing.began()

        let outcome2 = continuing.expired()
        #expect(outcome2 == [.notifyExpired, .complete(success: false)])
    }

    @Test func expiringWithNothingHeldOnlyTellsTheOwner() {
        var machine = HoldMachine(continues: true)

        let outcome1 = machine.expired()
        #expect(outcome1 == [.notifyExpired])
    }

    @Test func letGoWithNothingHeldDoesNothing() {
        var machine = HoldMachine(continues: true)

        let outcome1 = hold(&machine, false)
        #expect(outcome1.isEmpty)
    }

    @Test func aHoldAfterExpiryBeginsAfresh() {
        var machine = HoldMachine(continues: true)
        _ = hold(&machine, true)
        _ = machine.expired()

        let outcome1 = hold(&machine, true, id: "a.2")
        #expect(outcome1 == [.beginPlain, .submit("a.2")])
    }

    @Test func theShowsFlagFollowsTheSystemsTask() {
        var machine = HoldMachine(continues: true)
        _ = hold(&machine, true)

        #expect(!machine.shows())

        machine.submitted("a.1", accepted: true)
        _ = machine.began()
        #expect(machine.shows())
    }
}
