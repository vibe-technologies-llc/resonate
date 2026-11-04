import Foundation
import ResonateNative

public enum ListenedTime {
    public static func spoken(_ milliseconds: UInt64) -> String {
        let minutes = Int(milliseconds / 60_000)
        let hours = minutes / 60
        let rest = minutes % 60
        if hours == 0 { return counted(rest, "minute") }
        if rest == 0 { return counted(hours, "hour") }
        return "\(counted(hours, "hour")) \(counted(rest, "minute"))"
    }

    public static func brief(_ milliseconds: UInt64) -> String {
        let minutes = Int(milliseconds / 60_000)
        if minutes < 60 { return "\(minutes) min" }
        let hours = minutes / 60
        if hours < 100 { return "\(hours) h \(minutes % 60) min" }
        return "\(hours) h"
    }
}

public enum ListeningTrend: Equatable, Sendable {
    case steady
    case up(percent: Int)
    case down(percent: Int)
    case moreThanDouble

    private static let steadyWithinPercent = 5
    private static let doubledAtPercent = 100

    public static func of(now: UInt64, before: UInt64) -> ListeningTrend? {
        guard before > 0 else { return nil }
        let moved = (Double(now) - Double(before)) / Double(before) * 100
        let percent = Int(moved.rounded())
        if abs(percent) < steadyWithinPercent { return .steady }
        if percent > doubledAtPercent { return .moreThanDouble }
        return percent > 0 ? .up(percent: percent) : .down(percent: -percent)
    }

    public var words: String {
        switch self {
        case .steady: "about the same as the week before"
        case .up(let percent): "up \(percent)% on the week before"
        case .down(let percent): "down \(percent)% on the week before"
        case .moreThanDouble: "more than double the week before"
        }
    }
}

public enum ListeningGlanceArrangement: Sendable, Equatable {
    case stacked
    case beside

    public static func of(horizontal: SizeClass, accessibilityText: Bool) -> ListeningGlanceArrangement {
        horizontal == .regular && !accessibilityText ? .beside : .stacked
    }
}

public struct ListeningGlance: Equatable, Sendable {
    public enum Standing: Equatable, Sendable {
        case nothingHeard
        case heardThisWeek
        case restingThisWeek
    }

    public enum FactKind: Equatable, Sendable {
        case run
        case weekday
        case allTime
        case songsHeard
    }

    public struct Fact: Identifiable, Equatable, Sendable {
        public let kind: FactKind
        public let value: String
        public let caption: String
        public let spoken: String

        public var id: FactKind { kind }
    }

    public struct Day: Identifiable, Equatable, Sendable {
        public let starts: Date
        public let listenedMs: UInt64
        public let isToday: Bool

        public var id: Date { starts }
    }

    public struct Heard: Sendable {
        public let days: [ListenedOnDay]
        public let firstHeard: UInt64?
        public let plays: UInt64
        public let listenedMs: UInt64
        public let songsHeard: UInt64
        public let songs: UInt64

        public init(days: [ListenedOnDay], firstHeard: UInt64?, plays: UInt64, listenedMs: UInt64, songsHeard: UInt64, songs: UInt64) {
            self.days = days
            self.firstHeard = firstHeard
            self.plays = plays
            self.listenedMs = listenedMs
            self.songsHeard = songsHeard
            self.songs = songs
        }
    }

    public static let daysShown = 7
    public static let runShownFrom = 2
    public static let habitShownFromDaysHeard = 7
    private static let thisWeeksName = "Last 7 days"
    private static let allTimeName = "All time"
    private static let nothingHeardHeadline = "No plays yet"
    private static let nothingHeardDetail = "Play a song for half its length, or four minutes, to count a play."
    private static let daysOfTheWeek = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"]

    public let standing: Standing
    public let week: [Day]
    public let averageMs: UInt64
    public let eyebrow: String
    public let headline: String
    public let detail: String
    public let facts: [Fact]
    public let spoken: String

    public var tallestMs: UInt64 { week.map(\.listenedMs).max() ?? 0 }

    public static func unread(now: Date = .now, calendar: Calendar = .current) -> ListeningGlance {
        of(Heard(days: [], firstHeard: nil, plays: 0, listenedMs: 0, songsHeard: 0, songs: 0), now: now, calendar: calendar)
    }

    public static func of(_ heard: Heard, now: Date = .now, calendar: Calendar = .current) -> ListeningGlance {
        let today = calendar.startOfDay(for: now)
        let byDay = listenedByDay(heard.days, calendar: calendar)
        let week = lastDays(ending: today, calendar: calendar).map { day in
            Day(starts: day, listenedMs: byDay[day] ?? 0, isToday: day == today)
        }

        let thisWeekMs = week.reduce(0) { $0 + $1.listenedMs }
        let weekBeforeMs = lastDays(ending: calendar.date(byAdding: .day, value: -daysShown, to: today) ?? today, calendar: calendar)
            .reduce(0) { $0 + (byDay[$1] ?? 0) }
        let averageMs = thisWeekMs / UInt64(daysCovered(firstHeard: heard.firstHeard, today: today, calendar: calendar))
        let trend = ListeningTrend.of(now: thisWeekMs, before: weekBeforeMs)

        let run = runOfDays(byDay.keys, endingBy: today, calendar: calendar)
        let weekday = busiestWeekday(byDay, calendar: calendar)
        let lastHeard = byDay.keys.max()

        if byDay.isEmpty, heard.plays == 0 {
            return ListeningGlance(
                standing: .nothingHeard,
                week: week,
                averageMs: 0,
                eyebrow: thisWeeksName,
                headline: nothingHeardHeadline,
                detail: nothingHeardDetail,
                facts: [],
                spoken: "\(nothingHeardHeadline). \(nothingHeardDetail)"
            )
        }

        guard thisWeekMs > 0 else {
            let detail = lastHeard.map { "Nothing this week · last heard \(dated($0, today: today, calendar: calendar))" } ?? "Nothing this week"
            let facts = [
                weekdayFact(weekday),
                songsHeardFact(heard),
            ].compactMap { $0 }
            return ListeningGlance(
                standing: .restingThisWeek,
                week: week,
                averageMs: 0,
                eyebrow: allTimeName,
                headline: ListenedTime.brief(heard.listenedMs),
                detail: detail,
                facts: facts,
                spoken: said([
                    "All time, \(ListenedTime.spoken(heard.listenedMs)) listened, \(counted(Int(heard.plays), "play"))",
                    detail.replacingOccurrences(of: " · ", with: ", "),
                ] + facts.map(\.spoken))
            )
        }

        let pace = "\(ListenedTime.brief(averageMs)) a day"
        let facts = [
            runFact(run),
            weekdayFact(weekday),
            allTimeFact(heard),
            songsHeardFact(heard),
        ].compactMap { $0 }
        let spokenPace = "\(ListenedTime.spoken(averageMs)) a day" + (trend.map { ", \($0.words)" } ?? "")
        let busiest = week.max { $0.listenedMs < $1.listenedMs }.map { day in
            "The most was \(dayName(day.starts, calendar: calendar)), \(ListenedTime.spoken(day.listenedMs))"
        }
        return ListeningGlance(
            standing: .heardThisWeek,
            week: week,
            averageMs: averageMs,
            eyebrow: thisWeeksName,
            headline: ListenedTime.brief(thisWeekMs),
            detail: ([pace] + (trend.map { [$0.words] } ?? [])).joined(separator: " · "),
            facts: facts,
            spoken: said([
                "Last 7 days, \(ListenedTime.spoken(thisWeekMs)) listened",
                spokenPace,
                busiest,
            ].compactMap { $0 } + facts.map(\.spoken))
        )
    }

    private static func said(_ sentences: [String]) -> String {
        sentences.joined(separator: ". ")
    }

    private static func listenedByDay(_ days: [ListenedOnDay], calendar: Calendar) -> [Date: UInt64] {
        var byDay: [Date: UInt64] = [:]
        for day in days where day.listenedMs > 0 || day.plays > 0 {
            let starts = calendar.startOfDay(for: Date(timeIntervalSince1970: TimeInterval(day.starts)))
            byDay[starts, default: 0] += day.listenedMs
        }
        return byDay
    }

    private static func lastDays(ending last: Date, calendar: Calendar) -> [Date] {
        (0..<daysShown).reversed().compactMap { calendar.date(byAdding: .day, value: -$0, to: last) }
    }

    private static func daysCovered(firstHeard: UInt64?, today: Date, calendar: Calendar) -> Int {
        guard let firstHeard else { return daysShown }
        let first = calendar.startOfDay(for: Date(timeIntervalSince1970: TimeInterval(firstHeard)))
        let since = calendar.dateComponents([.day], from: first, to: today).day ?? daysShown
        return min(max(since + 1, 1), daysShown)
    }

    private static func runOfDays(_ heard: Dictionary<Date, UInt64>.Keys, endingBy today: Date, calendar: Calendar) -> Int {
        let yesterday = calendar.date(byAdding: .day, value: -1, to: today) ?? today
        guard var day = heard.contains(today) ? today : (heard.contains(yesterday) ? yesterday : nil) else { return 0 }

        var run = 0
        while heard.contains(day) {
            run += 1
            guard let before = calendar.date(byAdding: .day, value: -1, to: day) else { break }
            day = before
        }
        return run
    }

    private static func busiestWeekday(_ byDay: [Date: UInt64], calendar: Calendar) -> Int? {
        guard byDay.count >= habitShownFromDaysHeard else { return nil }

        var byWeekday: [Int: UInt64] = [:]
        for (day, listened) in byDay {
            byWeekday[calendar.component(.weekday, from: day), default: 0] += listened
        }
        return byWeekday.max { ($0.value, -$0.key) < ($1.value, -$1.key) }?.key
    }

    private static func dayName(_ day: Date, calendar: Calendar) -> String {
        daysOfTheWeek[calendar.component(.weekday, from: day) - 1]
    }

    private static func dated(_ day: Date, today: Date, calendar: Calendar) -> String {
        var style = Date.FormatStyle.dateTime.day().month(.abbreviated)
        if calendar.component(.year, from: day) != calendar.component(.year, from: today) {
            style = style.year()
        }
        style.calendar = calendar
        style.timeZone = calendar.timeZone
        style.locale = calendar.locale ?? .current
        return day.formatted(style)
    }

    private static func runFact(_ run: Int) -> Fact? {
        guard run >= runShownFrom else { return nil }
        return Fact(kind: .run, value: "\(run) days", caption: "in a row", spoken: "\(run) days in a row")
    }

    private static func weekdayFact(_ weekday: Int?) -> Fact? {
        guard let weekday else { return nil }
        let name = "\(daysOfTheWeek[weekday - 1])s"
        return Fact(kind: .weekday, value: name, caption: "heard most", spoken: "Heard most on \(name)")
    }

    private static func allTimeFact(_ heard: Heard) -> Fact {
        let plays = counted(Int(heard.plays), "play")
        return Fact(
            kind: .allTime,
            value: ListenedTime.brief(heard.listenedMs),
            caption: "in all · \(plays)",
            spoken: "\(ListenedTime.spoken(heard.listenedMs)) in all, \(plays)"
        )
    }

    private static func songsHeardFact(_ heard: Heard) -> Fact {
        let reading = heard.songsHeard <= heard.songs ? "\(heard.songsHeard) of \(heard.songs)" : "\(heard.songsHeard)"
        return Fact(kind: .songsHeard, value: reading, caption: "songs heard", spoken: "\(reading) songs heard")
    }
}
