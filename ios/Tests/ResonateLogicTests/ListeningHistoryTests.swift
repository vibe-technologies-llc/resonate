import Foundation
import ResonateNative
import Testing

@testable import ResonateLogic

@Suite struct ListeningHistoryTests {
    @Test func anImportSaysWhatLandedWhatWasHereAndWhatNamesSongsNotHere() {
        #expect(
            ListeningHistory.broughtIn(HistoryBroughtIn(credited: 1, heldAlready: 2, unmatched: 3))
                == "Brought in 1 listen · 2 already here · 3 of songs not on this phone"
        )
    }
}
