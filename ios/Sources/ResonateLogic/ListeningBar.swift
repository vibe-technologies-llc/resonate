import Foundation
import ResonateNative

public struct ListeningBar: Identifiable, Equatable, Sendable {
    public let starts: Date
    public let listenedMs: UInt64
    public let plays: UInt64

    public var id: Date { starts }

    public static func unit(for span: UInt32) -> Calendar.Component {
        switch span {
        case 0: .month
        case 365: .weekOfYear
        default: .day
        }
    }

    public static func grouped(_ days: [ListenedOnDay], span: UInt32, calendar: Calendar = .current) -> [ListeningBar] {
        let unit = unit(for: span)
        var bars: [Date: ListeningBar] = [:]
        for day in days {
            let moment = Date(timeIntervalSince1970: TimeInterval(day.starts))
            let key = calendar.dateInterval(of: unit, for: moment)?.start ?? moment
            let held = bars[key]
            bars[key] = ListeningBar(
                starts: key,
                listenedMs: (held?.listenedMs ?? 0) + day.listenedMs,
                plays: (held?.plays ?? 0) + day.plays
            )
        }
        return bars.values.sorted { $0.starts < $1.starts }
    }

    public static func edgeFormat(for span: UInt32) -> Date.FormatStyle {
        switch span {
        case 0: .dateTime.month(.abbreviated).year()
        case 365: .dateTime.day().month(.abbreviated).year()
        default: .dateTime.day().month(.abbreviated)
        }
    }

    public func caption(span: UInt32) -> String {
        switch span {
        case 0: starts.formatted(.dateTime.month(.wide).year())
        case 365: "Week of " + starts.formatted(.dateTime.day().month(.abbreviated))
        default: starts.formatted(.dateTime.weekday(.wide).day().month(.abbreviated))
        }
    }
}
