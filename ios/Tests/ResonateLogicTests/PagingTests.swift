import Testing

@testable import ResonateLogic

@Suite struct PagingTests {
    @Test func aSongArrivingWhileTheCatalogIsPagedStartsTheWalkAgainAndNoRowIsReadTwice() {
        var rows = Array(1...10)
        var revision: UInt64 = 0
        var walks = 0

        let read = Paging.steadily(revision: { revision }, key: { $0 }) {
            walks += 1
            let first = Array(rows.prefix(4))
            if walks == 1 {
                rows.insert(0, at: 0)
                revision += 1
            }
            return first + Array(rows.dropFirst(4))
        }

        #expect(read == Array(0...10))
        #expect(walks == 2)
    }

    @Test func aCatalogThatNeverStopsMovingIsStillReadOnceForEachRow() {
        var revision: UInt64 = 0

        let read = Paging.steadily(revision: { revision }, key: { $0 }) {
            revision += 1
            return [1, 2, 2, 3, 1]
        }

        #expect(read == [1, 2, 3])
    }
}
