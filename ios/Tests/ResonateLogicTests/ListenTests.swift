import Foundation
import ResonateNative
import Testing

@testable import ResonateLogic

func song(_ title: String, key: String? = nil) -> HeardSong {
    HeardSong(
        title: title,
        artist: "An artist",
        album: "",
        year: nil,
        isrc: "",
        recording: "",
        picture: nil,
        link: "",
        by: "Shazam",
        key: key ?? title.lowercased()
    )
}

@Suite struct ListenNextTests {
    @Test func aNamedSongIsToldWhenNothingStandsOrAnotherDoes() {
        let named = HeardAnswer.named(song: song("Echoes"))

        #expect(ListenNext.after(named, naming: nil, standing: nil) == .tell(song("Echoes")))
        #expect(ListenNext.after(named, naming: nil, standing: song("Hey You")) == .tell(song("Echoes")))
    }

    @Test func theSongAlreadyStandingIsKeptQuietlyWhileFollowing() {
        let named = HeardAnswer.named(song: song("Echoes", key: "echoes pink floyd"))

        #expect(ListenNext.after(named, naming: nil, standing: song("Echoes", key: "echoes pink floyd")) == .keep)
    }

    @Test func aSongNamedOnATryIsToldWhetherOrNotItStands() {
        let named = HeardAnswer.named(song: song("Echoes"))

        #expect(ListenNext.after(named, naming: 2, standing: song("Echoes")) == .tell(song("Echoes")))
    }

    @Test func aMissWhileFollowingChangesNothing() {
        for answer in [HeardAnswer.missed, .silent, .unreached(service: "Shazam")] {
            #expect(ListenNext.after(answer, naming: nil, standing: song("Echoes")) == .keep)
        }
    }

    @Test(arguments: [1, 2])
    func aMissOrASilentClipIsListenedToAgainUntilTheTriesRunOut(nth: Int) {
        #expect(ListenNext.after(.missed, naming: nth, standing: nil) == .again(nth: nth + 1))
        #expect(ListenNext.after(.silent, naming: nth, standing: nil) == .again(nth: nth + 1))
    }

    @Test func theLastTryThatMissesSettlesOnUnknownAndOnSilentOnSilent() {
        #expect(ListenNext.after(.missed, naming: ListenNext.triesOnAMiss, standing: nil) == .settle(.unknown))
        #expect(ListenNext.after(.silent, naming: ListenNext.triesOnAMiss, standing: nil) == .settle(.silent))
    }

    @Test func aServiceOutOfReachIsSaidAtOnceWithoutListeningAgain() {
        #expect(
            ListenNext.after(.unreached(service: "AudD"), naming: 1, standing: nil) == .settle(.unreached("AudD"))
        )
    }

    @Test func theStagesSayWhetherTheyListenAndWhetherTheyAnswered() {
        let listening: [ListenStage] = [.recording(nth: 1, since: Date()), .asking]
        let answered: [ListenStage] = [.found(song("Echoes")), .unknown]
        let neither: [ListenStage] = [.idle, .silent, .unreached("x"), .offline, .denied, .failed("x")]

        #expect(listening.allSatisfy { $0.isListening })
        #expect(answered.allSatisfy { $0.namedOrMissed })
        #expect(!(answered + neither).contains { $0.isListening })
        #expect(!(listening + neither).contains { $0.namedOrMissed })
    }
}

@Suite struct OnlyOnceTests {
    @Test func theFirstValueGivenBeforeAskingIsTheOneAsked() async {
        let once = OnlyOnce<Int>()

        once.give(1)
        once.give(2)

        #expect(await once.value() == 1)
    }

    @Test func aValueGivenWhileWaitingResumesTheWaiterWithTheFirstGiven() async {
        let once = OnlyOnce<Int>()
        let waiter = Task { await once.value() }
        try? await Task.sleep(for: .milliseconds(50))

        once.give(7)
        once.give(8)

        #expect(await waiter.value == 7)
    }

    @Test func aValueCanBeAskedForAgainAndIsTheSame() async {
        let once = OnlyOnce<String>()
        once.give("heard")

        #expect(await once.value() == "heard")
        #expect(await once.value() == "heard")
    }

    @Test func theWaiterIsNeverLostWhateverTheOrderOfAskingAndGiving() async {
        for round in 0..<200 {
            let once = OnlyOnce<Int>()
            let waiter = Task { await once.value() }
            let givers = Task {
                await withTaskGroup(of: Void.self) { group in
                    for value in 0..<4 { group.addTask { once.give(round * 10 + value) } }
                }
            }

            let heard = await waiter.value
            await givers.value

            #expect((round * 10..<round * 10 + 4).contains(heard))
        }
    }
}

@Suite struct ClipSinkTests {
    @Test func aClipFillsFromWhatIsAppendedAndStopsAtTheLengthWanted() async throws {
        let sink = ClipSink(wanted: 4)

        sink.append([1, 1, 1], sharing: 1)
        sink.append([1, 1, 1], sharing: 1)

        let heard = try await sink.filled()

        #expect(heard == [1, 1, 1, 1])
    }

    @Test func eachSampleIsScaledByItsShareOfTheChannels() async throws {
        let sink = ClipSink(wanted: 2)

        sink.append([2, 4], sharing: 0.5)

        let heard = try await sink.filled()

        #expect(heard == [1, 2])
    }

    @Test func aWaiterIsResumedAsTheLastSamplesArrive() async throws {
        let sink = ClipSink(wanted: 3)
        let waiter = Task { try await sink.filled() }
        try await Task.sleep(for: .milliseconds(50))

        sink.append([1, 1], sharing: 1)
        sink.append([1, 1], sharing: 1)

        let heard = try await waiter.value

        #expect(heard == [1, 1, 1])
    }

    @Test func finishingGivesWhatWasHeardEvenIfShort() async throws {
        let sink = ClipSink(wanted: 10)
        sink.append([3], sharing: 1)

        sink.finish()

        let heard = try await sink.filled()

        #expect(heard == [3])
    }

    @Test func finishingAWaiterResumesItWithWhatWasHeard() async throws {
        let sink = ClipSink(wanted: 10)
        let waiter = Task { try await sink.filled() }
        try await Task.sleep(for: .milliseconds(50))
        sink.append([3], sharing: 1)

        sink.finish()

        let heard = try await waiter.value

        #expect(heard == [3])
    }

    @Test func anInterruptionFailsTheClipForAWaiterAndForAnyoneAskingAfter() async throws {
        let sink = ClipSink(wanted: 10)
        let waiter = Task { try await sink.filled() }
        try await Task.sleep(for: .milliseconds(50))

        sink.interrupt()

        await #expect(throws: MicrophoneFailure.self) { try await waiter.value }
        await #expect(throws: MicrophoneFailure.self) { try await sink.filled() }
    }

    @Test func anInterruptionAfterTheClipIsFullChangesNothing() async throws {
        let sink = ClipSink(wanted: 1)
        sink.append([5], sharing: 1)

        sink.interrupt()

        let heard = try await sink.filled()

        #expect(heard == [5])
    }

    @Test func samplesAfterTheClipIsDoneAreIgnored() async throws {
        let sink = ClipSink(wanted: 1)
        sink.append([5], sharing: 1)

        sink.append([9], sharing: 1)

        let heard = try await sink.filled()

        #expect(heard == [5])
    }

    @Test func cancellingAWaiterStopsItAndLeavesTheNextToBeCancelledToo() async throws {
        let sink = ClipSink(wanted: 10)
        let waiter = Task { try await sink.filled() }
        try await Task.sleep(for: .milliseconds(50))

        waiter.cancel()

        await #expect(throws: CancellationError.self) { try await waiter.value }
        await #expect(throws: CancellationError.self) { try await sink.filled() }
    }

    @Test func aCancelledAskBeforeAnythingWaitsStillFailsWithCancellation() async {
        let sink = ClipSink(wanted: 10)
        let asked = Task {
            try await Task.sleep(for: .milliseconds(100))
            return try await sink.filled()
        }

        asked.cancel()

        await #expect(throws: Swift.Error.self) { try await asked.value }
    }
}
