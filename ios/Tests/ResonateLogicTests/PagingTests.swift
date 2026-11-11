import Testing

@testable import ResonateLogic

@Suite struct PagingTests {
    @Test func aWalkPinnedToItsFirstPageReadsOnceThoughSongsArriveBetweenPages() {
        var kept: [UInt64: [Int]] = [0: Array(1...10)]
        var revision: UInt64 = 0
        var pages = 0

        let read = Paging.pinned(pageRows: 4, key: { $0 }) { offset, pinned in
            pages += 1
            let at = pinned.flatMap { kept[$0] == nil ? nil : $0 } ?? revision
            let rows = Array(kept[at]!.dropFirst(Int(offset)).prefix(4))
            revision += 1
            kept[revision] = [-Int(revision)] + kept[revision - 1]!
            return (rows, at)
        }

        #expect(read == Array(1...10))
        #expect(pages == 3)
    }

    @Test func aWalkWhosePinWasLetGoStartsAgainOnTheCatalogAsItStands() {
        var rows = Array(1...10)
        var revision: UInt64 = 0
        var pages = 0

        let read = Paging.pinned(pageRows: 4, key: { $0 }) { offset, pinned in
            pages += 1
            let page = Array(rows.dropFirst(Int(offset)).prefix(4))
            let answered = revision
            if pages == 1 {
                rows.insert(0, at: 0)
                revision += 1
            }
            return (page, answered)
        }

        #expect(read == Array(0...10))
        #expect(pages == 5)
    }
}
