import Foundation
import Testing

@testable import ResonateLogic

@Suite struct TallyTests {
    struct Refused: LocalizedError {
        let errorDescription: String? = "It was refused"
    }

    @Test func aTallyOfNothingReadsZeroSongs() {
        let tally = Tally()

        #expect(tally.summary == "Read 0 songs")
        #expect(tally.count == 0)
        #expect(tally.complete)
    }

    @Test func songsReadAreCountedSingularAtOne() {
        let tally = Tally()

        _ = tally.indexed()
        #expect(tally.summary == "Read 1 song")
        _ = tally.indexed()
        #expect(tally.summary == "Read 2 songs")
        #expect(tally.foundSummary == "Found 2 new or changed songs")
        #expect(tally.stoppedSummary == "Stopped reading after 2 songs")
    }

    @Test func aCountIsPostedAtMostOnceAWhileAndTheFirstAlways() {
        let tally = Tally()

        #expect(tally.indexed() == 1)
        #expect(tally.indexed() == nil)
        #expect(tally.count == 2)
    }

    @Test func anUnreadableSongIsCountedAndNamedByItsFirstReason() {
        let tally = Tally()
        _ = tally.indexed()

        tally.failed("a.flac", Refused())
        tally.failed("b.flac", Refused())

        #expect(tally.summary == "Read 1 song; 2 unreadable (a.flac: It was refused)")
    }

    @Test func aFolderNotReachedMakesTheScanIncompleteAndIsNamedByItsFirstReason() {
        let tally = Tally()

        tally.skipped("Albums/Disc 2", Refused())

        #expect(!tally.complete)
        #expect(tally.summary == "Read 0 songs; 1 folder could not be read (Albums/Disc 2: It was refused)")
    }

    @Test func companionsLostAreCountedByKindAndJoinedWithAnd() {
        let tally = Tally()
        tally.lost(.cover, "cover.jpg", Refused())
        tally.lost(.cover, "other.jpg", Refused())
        tally.lost(.lyrics, "a.lrc", Refused())

        #expect(tally.summary == "Read 0 songs; 2 covers and 1 lyrics sheet could not be kept (cover.jpg: It was refused)")
    }

    @Test func eachKindOfTroubleIsToldInItsOwnClause() {
        let tally = Tally()
        _ = tally.indexed()
        tally.skipped("x", Refused())
        tally.failed("y", Refused())
        tally.lost(.lyrics, "z", Refused())

        #expect(
            tally.summary
                == "Read 1 song; 1 folder could not be read (x: It was refused); 1 unreadable (y: It was refused); 1 lyrics sheet could not be kept (z: It was refused)"
        )
    }
}
