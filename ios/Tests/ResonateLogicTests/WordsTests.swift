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

    @Test func aNumberIsCountedWithItsNounSingularOnlyAtOne() {
        #expect(counted(0, "song") == "0 songs")
        #expect(counted(1, "song") == "1 song")
        #expect(counted(2, "song") == "2 songs")
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
