import Foundation
import ResonateNative
import Testing

@testable import ResonateLogic

@Suite struct WordsTests {
    @Test(arguments: [
        (ResonateNative.Error.Player(message: "No speakers are plugged in"), "No speakers are plugged in"),
        (.Decode(message: "That file is damaged"), "That file is damaged"),
        (.Domain(message: "The folder is not a folder"), "The folder is not a folder"),
        (.Store(message: "The disk is full"), "The disk is full"),
        (.Closed(message: "The player is closed"), "The player is closed"),
    ])
    func aNativeFailureIsExplainedByTheSentenceItCarries(failure: ResonateNative.Error, sentence: String) {
        #expect(explained(failure) == sentence)
        #expect(explained(failure) != String(reflecting: failure))
    }

    @Test func aFailureThatIsNotNativeFallsBackToItsLocalisedDescription() {
        struct Plain: LocalizedError {
            var errorDescription: String? { "Nothing was found" }
        }

        #expect(explained(Plain()) == "Nothing was found")
    }

    @Test func aKeyIsSaidInTheEnglishItsStringsHold() {
        #expect(said("downloads.waiting-for-unmetered") == "Waiting for a network that is not metered")
    }

    @Test func aKeyTheStringsDoNotHoldIsSaidAsItself() {
        #expect(said("nowhere.held") == "nowhere.held")
    }

    @Test func aCountIsSaidSingularOnlyAtOne() {
        #expect(said("counted.songs", 0) == "0 songs")
        #expect(said("counted.songs", 1) == "1 song")
        #expect(said("counted.songs", 2) == "2 songs")
        #expect(said("counted.lyrics-sheets", 1) == "1 lyrics sheet")
    }

    @Test func aCountInASentenceTakesItsOwnArgumentWhereverTheOthersStand() {
        #expect(said("settings.folders.available", 1, 0) == "1 folder · 0 available")
        #expect(said("settings.folders.available", 3, 2) == "3 folders · 2 available")
        #expect(said("headphone-correction.replace.message", 1, "Flat") == "Flat replaces the 1 band you have now.")
        #expect(said("headphone-correction.replace.message", 10, "Flat") == "Flat replaces the 10 bands you have now.")
    }

    @Test func aLengthReadsAsMinutesAndSecondsAndAsHoursOnlyOnceItHasThem() {
        #expect(clock(0) == "0:00")
        #expect(clock(59_999) == "0:59")
        #expect(clock(61_000) == "1:01")
        #expect(clock(3_599_000) == "59:59")
        #expect(clock(3_600_000) == "1:00:00")
        #expect(clock(3_723_000) == "1:02:03")
    }
}
