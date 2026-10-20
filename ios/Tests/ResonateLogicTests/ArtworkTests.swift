import Foundation
import Testing

@testable import ResonateLogic

@Suite struct ArtworkBucketTests {
    @Test(arguments: [
        (CGFloat(48), CGFloat(2), ArtworkBucket.row),
        (48.1, 2, .tile),
        (96, 2, .tile),
        (96.1, 2, .card),
        (192, 2, .card),
        (256, 2, .shelf),
        (384, 2, .page),
        (512, 2, .full),
        (400, 3, .full),
        (10, 3, .row),
        (0, 3, .row),
        (2_000, 3, .full),
    ])
    func aSizeIsGivenTheSmallestBucketThatHoldsItsPixels(points: CGFloat, scale: CGFloat, bucket: ArtworkBucket) {
        #expect(ArtworkBucket.fitting(points: points, scale: scale) == bucket)
    }

    @Test func eachBucketIsAscendingSoTheFirstThatFitsIsTheSmallest() {
        let sizes = ArtworkBucket.allCases.map(\.rawValue)

        #expect(sizes == sizes.sorted())
        #expect(sizes == [96, 192, 384, 512, 768, 1024])
    }

    @Test func aFractionOfAPixelRoundsUpToTheNextWholeOne() {
        #expect(ArtworkBucket.fitting(points: 95.5, scale: 1) == .row)
        #expect(ArtworkBucket.fitting(points: 96.5, scale: 1) == .tile)
    }
}

@Suite struct ArtworkRequestTests {
    func request(_ subject: ArtworkSubject, bucket: ArtworkBucket = .tile) -> ArtworkRequest {
        ArtworkRequest(subject: subject, revision: 5, generation: 2, bucket: bucket)
    }

    @Test func aTracksNameHoldsItsRevisionGenerationAndBucket() {
        #expect(request(.track(9)).name as String == "track:9:5:2:192")
    }

    @Test func aPortraitsNameHoldsItsGenerationAndBucketButNotItsRevision() {
        #expect(request(.portrait("abc")).name as String == "portrait:abc:2:192")
    }

    @Test func aPictureIsNamedByItsIdAndBucketAlone() {
        #expect(request(.picture(7)).name as String == "picture:7:192")
        #expect(request(.picture(7), bucket: .card).name as String == "picture:7:384")
    }

    @Test func theSongNameIgnoresTheBucketSoEveryBucketOfASongSharesItsPicture() {
        #expect(request(.track(9)).songName == request(.track(9), bucket: .full).songName)
        #expect(request(.track(9)).songName as String == "song:5:2:track:9")
    }

    @Test func aTracksPictureIsRequestedWithoutRevisionOrGeneration() {
        let picture = request(.track(9), bucket: .shelf).of(picture: 41)

        #expect(picture.subject == .picture(41))
        #expect(picture.revision == 0)
        #expect(picture.generation == 0)
        #expect(picture.bucket == .shelf)
        #expect(picture.name as String == "picture:41:512")
    }

    @Test func twoRequestsDifferingInAnyPartAreDifferentFlights() {
        let base = request(.track(9))

        #expect(base == request(.track(9)))
        #expect(base != request(.track(10)))
        #expect(base != request(.track(9), bucket: .card))
        #expect(base != ArtworkRequest(subject: .track(9), revision: 6, generation: 2, bucket: .tile))
        #expect(base != ArtworkRequest(subject: .track(9), revision: 5, generation: 3, bucket: .tile))
    }
}

@Suite struct DecodeSlotsTests {
    actor Order {
        private(set) var granted: [Int] = []

        func grant(_ id: Int) { granted.append(id) }
    }

    @Test func slotsAreGivenUpToTheCountAndTheNextWaitsForARelease() async throws {
        let slots = DecodeSlots(2)
        let order = Order()
        try await slots.acquire()
        try await slots.acquire()

        let third = Task {
            try await slots.acquire()
            await order.grant(3)
        }
        try await Task.sleep(for: .milliseconds(50))
        #expect(await order.granted.isEmpty)

        await slots.release()
        try await third.value
        #expect(await order.granted == [3])
    }

    @Test func waitersAreGrantedInTheOrderTheyAsked() async throws {
        let slots = DecodeSlots(1)
        let order = Order()
        try await slots.acquire()

        var waiters: [Task<Void, any Swift.Error>] = []
        for id in 1...3 {
            waiters.append(
                Task {
                    try await slots.acquire()
                    await order.grant(id)
                }
            )
            try await Task.sleep(for: .milliseconds(30))
        }
        for _ in 1...3 { await slots.release() }
        for waiter in waiters { try await waiter.value }

        #expect(await order.granted == [1, 2, 3])
    }

    @Test func aWaiterCancelledWhileWaitingLeavesWithoutTakingASlot() async throws {
        let slots = DecodeSlots(1)
        try await slots.acquire()
        let waiter = Task { try await slots.acquire() }
        try await Task.sleep(for: .milliseconds(50))

        waiter.cancel()

        await #expect(throws: CancellationError.self) { try await waiter.value }
        await slots.release()
        try await slots.acquire()
    }

    @Test func aTaskCancelledBeforeAskingIsRefusedAtOnce() async {
        let slots = DecodeSlots(1)
        let asked = Task {
            try await Task.sleep(for: .milliseconds(100))
            try await slots.acquire()
        }

        asked.cancel()

        await #expect(throws: Swift.Error.self) { try await asked.value }
    }

    @Test func releasesWithNobodyWaitingAddSlotsBack() async throws {
        let slots = DecodeSlots(1)
        try await slots.acquire()
        await slots.release()

        try await slots.acquire()
    }
}

@Suite struct FlightsTests {
    let request = ArtworkRequest(subject: .track(1), revision: 0, generation: 0, bucket: .row)
    let other = ArtworkRequest(subject: .track(2), revision: 0, generation: 0, bucket: .row)

    func decode() -> Task<Int?, Never> {
        Task {
            do {
                try await Task.sleep(for: .seconds(30))
                return 1
            } catch {
                return nil
            }
        }
    }

    @Test func askersOfOneRequestJoinTheFlightTheFirstStarted() {
        var flights = Flights<Int>()
        var started = 0

        let first = flights.join(request) { started += 1; return decode() }
        let second = flights.join(request) { started += 1; return decode() }

        #expect(started == 1)
        #expect(first == second)
        #expect(flights.count == 1)
        first.cancel()
    }

    @Test func differentRequestsFlyApart() {
        var flights = Flights<Int>()

        let first = flights.join(request, starting: decode)
        let second = flights.join(other, starting: decode)

        #expect(first != second)
        #expect(flights.count == 2)
        first.cancel()
        second.cancel()
    }

    @Test func aFlightKeepsGoingUntilTheLastAskerLeaves() async {
        var flights = Flights<Int>()
        let task = flights.join(request, starting: decode)
        _ = flights.join(request, starting: decode)

        flights.leave(request, task)
        #expect(!task.isCancelled)
        #expect(flights.count == 1)

        flights.leave(request, task)
        #expect(task.isCancelled)
        #expect(flights.count == 0)
        #expect(await task.value == nil)
    }

    @Test func aFlightSettledIsForgottenWithoutBeingCancelled() {
        var flights = Flights<Int>()
        let task = flights.join(request, starting: decode)

        flights.settle(request, task)

        #expect(flights.count == 0)
        #expect(!task.isCancelled)
        task.cancel()
    }

    @Test func aStaleTaskLeavingOrSettlingTouchesNeitherTheFlightThatReplacedIt() {
        var flights = Flights<Int>()
        let stale = flights.join(request, starting: decode)
        flights.settle(request, stale)
        let fresh = flights.join(request, starting: decode)

        flights.leave(request, stale)
        flights.settle(request, stale)

        #expect(!fresh.isCancelled)
        #expect(flights.count == 1)
        stale.cancel()
        fresh.cancel()
    }
}
