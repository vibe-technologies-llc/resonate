import Testing

@testable import ResonateLogic

@Suite struct FastScrollTests {
    @Test func theThumbIsOfferedFromSixtyRowsAsOnAndroid() {
        #expect(!FastScroll.offered(rows: 59))
        #expect(FastScroll.offered(rows: 60))
    }

    @Test func theScrolledShareIsTheOffsetOverTheTravelHeldWithinTheWhole() {
        #expect(FastScroll.scrolled(offset: 0, content: 2_000, container: 1_000) == 0)
        #expect(FastScroll.scrolled(offset: 500, content: 2_000, container: 1_000) == 0.5)
        #expect(FastScroll.scrolled(offset: 1_000, content: 2_000, container: 1_000) == 1)
        #expect(FastScroll.scrolled(offset: -80, content: 2_000, container: 1_000) == 0)
        #expect(FastScroll.scrolled(offset: 1_400, content: 2_000, container: 1_000) == 1)
        #expect(FastScroll.scrolled(offset: 10, content: 400, container: 1_000) == 0)
    }

    @Test func aShareNamesARowAndTheRowGivesTheShareBack() {
        #expect(FastScroll.row(at: 0, of: 1_001) == 0)
        #expect(FastScroll.row(at: 0.5, of: 1_001) == 500)
        #expect(FastScroll.row(at: 1, of: 1_001) == 1_000)
        #expect(FastScroll.row(at: 2, of: 1_001) == 1_000)
        #expect(FastScroll.row(at: 0.5, of: 1) == 0)
        #expect(FastScroll.row(at: 0.5, of: 0) == 0)
    }

    @Test func aWindowsScrollIsPlacedWithinTheWholeList() {
        #expect(FastScroll.share(of: 0..<300, in: 10_000, scrolled: 0) == 0)
        #expect(FastScroll.share(of: 9_700..<10_000, in: 10_000, scrolled: 1) == 1)

        let middle = FastScroll.share(of: 4_900..<5_200, in: 10_000, scrolled: 0.5)

        #expect(abs(middle - 0.505) < 0.001)
        #expect(FastScroll.share(of: 0..<1, in: 1, scrolled: 0.5) == 0)
    }

    @Test func theThumbTravelsTheTrackLessItsOwnLength() {
        #expect(FastScroll.thumbOffset(share: 0, track: 552) == 0)
        #expect(FastScroll.thumbOffset(share: 0.5, track: 552) == 250)
        #expect(FastScroll.thumbOffset(share: 1, track: 552) == 500)
        #expect(FastScroll.thumbOffset(share: 1, track: 20) == 0)
    }

    @Test func aDragMovesTheShareByItsPartOfTheTravel() {
        #expect(FastScroll.share(draggedFrom: 0.2, by: 250, track: 552) == 0.7)
        #expect(FastScroll.share(draggedFrom: 0.2, by: -500, track: 552) == 0)
        #expect(FastScroll.share(draggedFrom: 0.9, by: 500, track: 552) == 1)
        #expect(FastScroll.share(draggedFrom: 0.4, by: 100, track: 20) == 0.4)
    }

    @Test func anAdjustmentStepsATenthAndStopsAtTheEnds() {
        #expect(abs(FastScroll.adjusted(0.5, forward: true) - 0.6) < 0.0001)
        #expect(abs(FastScroll.adjusted(0.5, forward: false) - 0.4) < 0.0001)
        #expect(FastScroll.adjusted(0.95, forward: true) == 1)
        #expect(FastScroll.adjusted(0.05, forward: false) == 0)
    }

    @Test func aTouchThatHasNotMovedDragsNothing() {
        #expect(!FastScroll.moved(by: 0))
        #expect(!FastScroll.moved(by: -3.9))
        #expect(FastScroll.moved(by: 4))
        #expect(FastScroll.moved(by: -12))
    }
}
