import ResonateNative
import Testing

@testable import ResonateLogic

@Suite struct PollingTests {
    @Test func aQueueNeverReadIsDue() {
        #expect(QueueListed(snapshot()).isDue(after: nil, since: .now))
    }

    @Test func aMovedQueueRevisionIsDueAtOnce() {
        let read = QueueListed(snapshot(queueRevision: 1))
        let moved = QueueListed(snapshot(queueRevision: 2))

        #expect(moved.isDue(after: read, since: .now))
    }

    @Test func anotherZoneShownIsDueAtOnceThoughItsRevisionIsTheSame() {
        let read = QueueListed(snapshot(zone: 1))
        let other = QueueListed(snapshot(zone: 2))

        #expect(other.isDue(after: read, since: .now))
    }

    @Test func anUnmovedQueueIsNotDueHoweverLongAgoItWasRead() {
        let read = QueueListed(snapshot())

        #expect(!QueueListed(snapshot()).isDue(after: read, since: .now - .seconds(60)))
    }

    @Test func aMovedCatalogIsDueOnlyOnceTheLastReadIsOldEnough() {
        let read = QueueListed(snapshot(catalogRevision: 1))
        let moved = QueueListed(snapshot(catalogRevision: 2))

        #expect(!moved.isDue(after: read, since: .now - .seconds(1)))
        #expect(moved.isDue(after: read, since: .now - QueueListed.catalogRereadAfter - .milliseconds(1)))
    }

    @Test func aSettlingStandsSettledOnlyWhileEveryAskIsAnswered() {
        var settling = Settling()
        #expect(settling.settled)

        settling.ask()
        settling.ask()
        #expect(!settling.settled)

        settling.answer()
        #expect(!settling.settled)

        settling.answer()
        #expect(settling.settled)
        #expect(settling.asked == 2)
        #expect(settling.answered == 2)
    }

    @Test func nothingSteeredLeavesTheSnapshotAsItWas() {
        let state = snapshot(.playing, current: track(1), positionMs: 5_000, durationMs: 200_000)

        #expect(Steered().applied(to: state) == state)
    }

    @Test func aSteeredTransportShuffleAndRepeatReplaceTheSnapshotsAndNothingElse() {
        var steered = Steered()
        steered.transport = .paused
        steered.shuffle = true
        steered.repeatMode = 2
        let state = snapshot(.playing, current: track(1), positionMs: 5_000, durationMs: 200_000)

        let shown = steered.applied(to: state)

        #expect(shown.transport == .paused)
        #expect(shown.shuffle)
        #expect(shown.repeat == 2)
        #expect(shown.positionMs == 5_000)
        #expect(shown.current == state.current)
    }

    @Test func aSteeredSongIsShownFromItsStartWithItsOwnLength() {
        var steered = Steered()
        steered.song = SteeredSong(track: track(7, durationMs: 90_000), index: 4)
        let state = snapshot(.playing, current: track(1), queueIndex: 1, positionMs: 5_000, durationMs: 200_000)

        let shown = steered.applied(to: state)

        #expect(shown.current?.id == 7)
        #expect(shown.queueIndex == 4)
        #expect(shown.positionMs == 0)
        #expect(shown.durationMs == 90_000)
    }

    @Test func aRestartedSongReturnsToItsStartAndKeepsItsLength() {
        var steered = Steered()
        steered.restarted = true
        let state = snapshot(.playing, current: track(1), positionMs: 50_000, durationMs: 200_000)

        let shown = steered.applied(to: state)

        #expect(shown.positionMs == 0)
        #expect(shown.durationMs == 200_000)
        #expect(shown.current?.id == 1)
    }

    @Test func aSteeredSongOutranksARestart() {
        var steered = Steered()
        steered.restarted = true
        steered.song = SteeredSong(track: track(7), index: 2)

        #expect(steered.applied(to: snapshot(positionMs: 1_000)).current?.id == 7)
    }

    @Test(arguments: [
        (true, Transport.playing, 100_000, false, Polling.busy),
        (true, .loading, 100_000, false, Polling.busy),
        (true, .playing, 199_000, false, Polling.nearEnd),
        (true, .loading, 199_000, false, Polling.busy),
        (true, .paused, 0, false, Polling.idle),
        (true, .paused, 0, true, Polling.busy),
        (false, .playing, 100_000, false, Polling.background),
        (false, .paused, 0, true, Polling.background),
    ])
    func theWaitBetweenPollsFollowsTheSceneAndWhatMoves(
        foreground: Bool,
        transport: Transport,
        positionMs: UInt64,
        analysing: Bool,
        wait: Duration
    ) {
        let state = snapshot(transport, positionMs: positionMs, durationMs: 200_000)

        #expect(Polling.interval(foreground: foreground, state: state, analysing: analysing, asked: false) == wait)
    }

    @Test func aCommandJustAskedKeepsTheBusyPaceWhateverElseSays() {
        let state = snapshot(.paused)

        #expect(Polling.interval(foreground: false, state: state, analysing: false, asked: true) == Polling.busy)
    }

    @Test func aSongIsEndingSoonOnlyInTheLastMomentsOfAKnownLength() {
        #expect(Polling.endingSoon(snapshot(.playing, positionMs: 198_500, durationMs: 200_000)))
        #expect(!Polling.endingSoon(snapshot(.playing, positionMs: 198_499, durationMs: 200_000)))
        #expect(!Polling.endingSoon(snapshot(.playing, positionMs: 5, durationMs: 0)))
        #expect(!Polling.endingSoon(snapshot(.paused, positionMs: 199_000, durationMs: 200_000)))
        #expect(!Polling.endingSoon(snapshot(.playing, positionMs: 200_000, durationMs: 200_000)))
    }

    @Test func twoSnapshotsDifferingOnlyInWhatMovesEveryTickAreSteadyAlike() {
        let one = snapshot(.playing, current: track(1), positionMs: 1_000, durationMs: 200_000, queueRevision: 3, catalogRevision: 4)
        var other = snapshot(.playing, current: track(1), positionMs: 9_000, durationMs: 200_000, queueRevision: 5, catalogRevision: 6)
        other.outputRate = 48_000
        other.underruns = 3
        other.overs = 41
        other.failure = "A failure"

        #expect(Polling.steady(one) == Polling.steady(other))
    }

    @Test func aSnapshotThatChangedTransportOrSongIsNotSteadyAlike() {
        let one = snapshot(.playing, current: track(1))

        #expect(Polling.steady(one) != Polling.steady(snapshot(.paused, current: track(1))))
        #expect(Polling.steady(one) != Polling.steady(snapshot(.playing, current: track(2))))
        #expect(Polling.steady(one) != Polling.steady(snapshot(.playing, current: track(1), shuffle: true)))
    }

    @Test func aSleepTimerCountingDownIsSteadyButOneThatIsSetOrClearedIsNot() {
        var running = snapshot(.playing)
        running.sleepRemainingMs = 50_000
        var later = running
        later.sleepRemainingMs = 40_000

        #expect(Polling.steady(running) == Polling.steady(later))
        #expect(Polling.steady(running) != Polling.steady(snapshot(.playing)))
    }

    @Test func previousRestartsTheSongOnlyPastItsOpeningOfALongEnoughSong() {
        #expect(Steering.previousRestarts(positionMs: 3_001, durationMs: 200_000))
        #expect(!Steering.previousRestarts(positionMs: 3_000, durationMs: 200_000))
        #expect(!Steering.previousRestarts(positionMs: 3_001, durationMs: 3_000))
        #expect(!Steering.previousRestarts(positionMs: 0, durationMs: 0))
    }

    @Test func aSkipLandsOnTheNeighbouringRowAndNamesItsPlace() {
        let queue = [track(1), track(2), track(3)]

        #expect(Steering.neighbour(of: 1, step: 1, in: queue, repeating: false) == SteeredSong(track: queue[2], index: 2))
        #expect(Steering.neighbour(of: 1, step: -1, in: queue, repeating: false) == SteeredSong(track: queue[0], index: 0))
    }

    @Test func nextPastTheEndWrapsToTheFirstRowOnlyUnderARepeat() {
        let queue = [track(1), track(2)]

        #expect(Steering.neighbour(of: 1, step: 1, in: queue, repeating: false) == nil)
        #expect(Steering.neighbour(of: 1, step: 1, in: queue, repeating: true) == SteeredSong(track: queue[0], index: 0))
    }

    @Test func previousBeforeTheStartNeverWraps() {
        let queue = [track(1), track(2)]

        #expect(Steering.neighbour(of: 0, step: -1, in: queue, repeating: true) == nil)
    }

    @Test func noPlayingRowOrNoQueueSteersNothing() {
        #expect(Steering.neighbour(of: nil, step: 1, in: [track(1)], repeating: true) == nil)
        #expect(Steering.neighbour(of: 0, step: 1, in: [], repeating: true) == nil)
    }
}
