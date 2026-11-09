import Foundation
import ResonateNative

public struct WrappedCardRow: Equatable, Sendable {
    public let label: String
    public let name: String
    public let by: String?
}

public struct WrappedCardWords: Equatable, Sendable {
    public static let rowsAtMost = 4

    public let eyebrow: String
    public let headline: String
    public let detail: String?
    public let rows: [WrappedCardRow]

    public static func of(_ year: WrappedYear) -> WrappedCardWords? {
        guard let whole = year.slides.first else { return nil }
        let ranked = year.slides.dropFirst().filter { !$0.rows.isEmpty }.prefix(rowsAtMost)
        return WrappedCardWords(
            eyebrow: whole.eyebrow,
            headline: whole.headline,
            detail: whole.detail,
            rows: ranked.map { WrappedCardRow(label: $0.eyebrow, name: $0.headline, by: $0.rows.first?.by) }
        )
    }
}

public enum WrappedDeckPlace {
    private static let hoursLabelledEvery = 6
    private static let monthsLabelledEvery = 1
    private static let monthsLabelledEveryWhenLarge = 3
    private static let barsInAYear = 12

    public static func turned(from place: Int, by step: Int, of count: Int) -> Int {
        min(max(place + step, 0), max(count - 1, 0))
    }

    public static func said(at place: Int, of count: Int) -> String {
        ResonateLogic.said("wrapped.deck.place", place + 1, count)
    }

    public static func labelledEvery(bars: Int, accessibilityText: Bool) -> Int {
        if bars > barsInAYear { return hoursLabelledEvery }
        return accessibilityText ? monthsLabelledEveryWhenLarge : monthsLabelledEvery
    }
}
