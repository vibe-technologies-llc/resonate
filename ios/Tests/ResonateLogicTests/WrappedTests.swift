import Foundation
import ResonateNative
import Testing

@testable import ResonateLogic

private func slide(_ eyebrow: String, _ headline: String, by: String? = nil, ranked: Bool = false) -> WrappedSlide {
    WrappedSlide(
        eyebrow: eyebrow,
        headline: headline,
        detail: nil,
        rows: ranked ? [WrappedRow(name: headline, by: by, figure: "1 hour")] : [],
        bars: []
    )
}

private func year(_ slides: [WrappedSlide]) -> WrappedYear {
    WrappedYear(year: 2025, years: [2025], slides: slides, summary: "My 2025 on Resonate")
}

@Suite struct WrappedCardWordsTests {
    @Test func theCardLeadsWithTheFirstSlideAndRanksOnlyTheSlidesThatRank() throws {
        let words = try #require(WrappedCardWords.of(year([
            slide("Resonate Wrapped 2025", "100 hours of music"),
            slide("Your song of the year", "Echoes", by: "Pink Floyd", ranked: true),
            slide("Your year", "March was your biggest month"),
            slide("Your artist of the year", "Air", ranked: true),
        ])))

        #expect(words.eyebrow == "Resonate Wrapped 2025")
        #expect(words.headline == "100 hours of music")
        #expect(words.rows == [
            WrappedCardRow(label: "Your song of the year", name: "Echoes", by: "Pink Floyd"),
            WrappedCardRow(label: "Your artist of the year", name: "Air", by: nil),
        ])
    }

    @Test func theCardHoldsAtMostFourRows() throws {
        let ranked = (1...6).map { slide("Top \($0)", "Name \($0)", ranked: true) }
        let words = try #require(WrappedCardWords.of(year([slide("E", "H")] + ranked)))

        #expect(words.rows.map(\.name) == ["Name 1", "Name 2", "Name 3", "Name 4"])
    }

    @Test func aYearWithNoSlideDrawsNoCard() {
        #expect(WrappedCardWords.of(year([])) == nil)
    }
}

@Suite struct WrappedDeckPlaceTests {
    @Test func aTurnStopsAtBothEnds() {
        #expect(WrappedDeckPlace.turned(from: 0, by: -1, of: 3) == 0)
        #expect(WrappedDeckPlace.turned(from: 0, by: 1, of: 3) == 1)
        #expect(WrappedDeckPlace.turned(from: 2, by: 1, of: 3) == 2)
        #expect(WrappedDeckPlace.turned(from: 0, by: 1, of: 0) == 0)
    }

    @Test func thePlaceCountsFromOne() {
        #expect(WrappedDeckPlace.said(at: 0, of: 12) == "1 of 12")
    }

    @Test func aDaysHoursAreLabelledSparselyAndAYearsMonthsAllUnlessTheTextIsLarge() {
        #expect(WrappedDeckPlace.labelledEvery(bars: 24, accessibilityText: false) == 6)
        #expect(WrappedDeckPlace.labelledEvery(bars: 12, accessibilityText: false) == 1)
        #expect(WrappedDeckPlace.labelledEvery(bars: 12, accessibilityText: true) == 3)
    }
}
