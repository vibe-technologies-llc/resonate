import Foundation
import ResonateNative
import Testing

@testable import ResonateLogic

@Suite struct HomeTests {
    @Test func theGreetingFollowsTheHourAsAndroidsDoes() {
        #expect(HomeGreeting.of(hour: 5) == "Good morning")
        #expect(HomeGreeting.of(hour: 11) == "Good morning")
        #expect(HomeGreeting.of(hour: 12) == "Good afternoon")
        #expect(HomeGreeting.of(hour: 17) == "Good afternoon")
        #expect(HomeGreeting.of(hour: 18) == "Good evening")
        #expect(HomeGreeting.of(hour: 0) == "Good evening")
        #expect(HomeGreeting.of(hour: 4) == "Good evening")
    }
}

@Suite struct ListeningGlanceTests {
    private static let hourMs: UInt64 = 3_600_000
    private static let minuteMs: UInt64 = 60_000

    private static let calendar: Calendar = {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = TimeZone(identifier: "UTC") ?? .current
        calendar.locale = Locale(identifier: "en_US_POSIX")
        return calendar
    }()

    private static let thursdayNoon = calendar.date(from: DateComponents(year: 2026, month: 11, day: 5, hour: 12)) ?? .now
    private static let today = calendar.startOfDay(for: thursdayNoon)

    private func day(_ back: Int, listenedMs: UInt64, plays: UInt64 = 1) -> ListenedOnDay {
        let moment = Self.calendar.date(byAdding: .day, value: -back, to: Self.today) ?? Self.today
        return ListenedOnDay(starts: UInt64(moment.timeIntervalSince1970), plays: plays, listenedMs: listenedMs)
    }

    private func glance(
        _ days: [ListenedOnDay],
        firstHeardBack: Int = 400,
        plays: UInt64 = 120,
        listenedMs: UInt64 = 300 * hourMs,
        songsHeard: UInt64 = 40,
        songs: UInt64 = 100
    ) -> ListeningGlance {
        let first = Self.calendar.date(byAdding: .day, value: -firstHeardBack, to: Self.today) ?? Self.today
        return ListeningGlance.of(
            ListeningGlance.Heard(
                days: days,
                firstHeard: days.isEmpty ? nil : UInt64(first.timeIntervalSince1970),
                plays: plays,
                listenedMs: listenedMs,
                songsHeard: songsHeard,
                songs: songs
            ),
            now: Self.thursdayNoon,
            calendar: Self.calendar
        )
    }

    @Test func timeListenedReadsBrieflyAndAloud() {
        #expect(ListenedTime.brief(5 * Self.minuteMs) == "5 min")
        #expect(ListenedTime.brief(6 * Self.hourMs + 12 * Self.minuteMs) == "6 h 12 min")
        #expect(ListenedTime.brief(312 * Self.hourMs) == "312 h")
        #expect(ListenedTime.spoken(Self.minuteMs) == "1 minute")
        #expect(ListenedTime.spoken(2 * Self.hourMs) == "2 hours")
        #expect(ListenedTime.spoken(Self.hourMs + 3 * Self.minuteMs) == "1 hour 3 minutes")
    }

    @Test func aTrendIsTheWeekAgainstTheOneBeforeWithNothingToCompareSaidAsNothing() {
        #expect(ListeningTrend.of(now: 100, before: 0) == nil)
        #expect(ListeningTrend.of(now: 103, before: 100) == .steady)
        #expect(ListeningTrend.of(now: 120, before: 100) == .up(percent: 20))
        #expect(ListeningTrend.of(now: 85, before: 100) == .down(percent: 15))
        #expect(ListeningTrend.of(now: 200, before: 100) == .up(percent: 100))
        #expect(ListeningTrend.of(now: 201, before: 100) == .moreThanDouble)
        #expect(ListeningTrend.up(percent: 20).words == "up 20% on the week before")
    }

    @Test func someoneWhoHasPlayedNothingIsToldHowAPlayCountsOverSevenEmptyDays() {
        let empty = glance([], plays: 0, listenedMs: 0, songsHeard: 0)

        #expect(empty.standing == .nothingHeard)
        #expect(empty.headline == "No plays yet")
        #expect(empty.detail.contains("half its length"))
        #expect(empty.week.count == ListeningGlance.daysShown)
        #expect(empty.tallestMs == 0)
        #expect(empty.averageMs == 0)
        #expect(empty.facts.isEmpty)
        #expect(empty.spoken.hasPrefix("No plays yet."))
    }

    @Test func sevenDaysEndingTodayAreDrawnWithGapsFilledAndTodayMarked() {
        let week = glance([day(0, listenedMs: Self.hourMs), day(2, listenedMs: 2 * Self.hourMs), day(7, listenedMs: Self.hourMs)]).week

        #expect(week.count == 7)
        #expect(week.last?.starts == Self.today)
        #expect(week.last?.isToday == true)
        #expect(week.filter(\.isToday).count == 1)
        #expect(week.map(\.listenedMs) == [0, 0, 0, 0, 2 * Self.hourMs, 0, Self.hourMs])
    }

    @Test func theHeadlineIsTheWeeksTimeWithItsPaceAndTrend() {
        let heard = glance([
            day(0, listenedMs: 4 * Self.hourMs),
            day(3, listenedMs: 3 * Self.hourMs),
            day(8, listenedMs: 3 * Self.hourMs + 30 * Self.minuteMs),
        ])

        #expect(heard.standing == .heardThisWeek)
        #expect(heard.eyebrow == "Last 7 days")
        #expect(heard.headline == "7 h 0 min")
        #expect(heard.averageMs == Self.hourMs)
        #expect(heard.detail == "1 h 0 min a day · up 100% on the week before")
    }

    @Test func aNewListenersPaceIsOverTheDaysSinceTheFirstPlay() {
        let new = glance([day(0, listenedMs: 2 * Self.hourMs), day(1, listenedMs: 2 * Self.hourMs)], firstHeardBack: 1)

        #expect(new.averageMs == 2 * Self.hourMs)
        #expect(new.detail == "2 h 0 min a day")
    }

    @Test func aRunEndsTodayOrYesterdayAndOnlyTwoOrMoreDaysAreShown() {
        let kept = glance([day(0, listenedMs: 1), day(1, listenedMs: 1), day(2, listenedMs: 1), day(4, listenedMs: 1)])
        let yesterdays = glance([day(1, listenedMs: Self.hourMs), day(2, listenedMs: Self.hourMs)])
        let broken = glance([day(0, listenedMs: Self.hourMs), day(2, listenedMs: Self.hourMs)])

        #expect(kept.facts.first { $0.kind == .run }?.value == "3 days")
        #expect(yesterdays.facts.first { $0.kind == .run }?.spoken == "2 days in a row")
        #expect(broken.facts.first { $0.kind == .run } == nil)
    }

    @Test func theWeekdayHeardMostWaitsForAWeekOfHeardDays() {
        let few = glance([day(0, listenedMs: Self.hourMs)])
        let mondays = [3, 10].map { day($0, listenedMs: 5 * Self.hourMs) }
        let rest = [0, 1, 2, 4, 5].map { day($0, listenedMs: Self.hourMs) }
        let habit = glance(mondays + rest)

        #expect(few.facts.first { $0.kind == .weekday } == nil)
        #expect(habit.facts.first { $0.kind == .weekday }?.value == "Mondays")
        #expect(habit.facts.first { $0.kind == .weekday }?.spoken == "Heard most on Mondays")
    }

    @Test func aListenerAwayThisWeekReadsAllTimeAndWhenLastHeardWithNoChart() {
        let away = glance([day(30, listenedMs: Self.hourMs)])

        #expect(away.standing == .restingThisWeek)
        #expect(away.eyebrow == "All time")
        #expect(away.headline == "300 h")
        #expect(away.detail == "Nothing this week · last heard Oct 6")
        #expect(away.averageMs == 0)
        #expect(away.facts.map(\.kind) == [.songsHeard])
        #expect(away.spoken.contains("last heard Oct 6"))
    }

    @Test func aLastHeardDayOfAnotherYearSaysTheYear() {
        let away = glance([day(320, listenedMs: Self.hourMs)])

        #expect(away.detail == "Nothing this week · last heard Dec 20, 2025")
    }

    @Test func songsHeardSayTheLibraryUnlessServedSongsPassIt() {
        let held = glance([day(0, listenedMs: Self.hourMs)])
        let served = glance([day(0, listenedMs: Self.hourMs)], songsHeard: 150, songs: 100)

        #expect(held.facts.first { $0.kind == .songsHeard }?.value == "40 of 100")
        #expect(served.facts.first { $0.kind == .songsHeard }?.value == "150")
    }

    @Test func everyFigureIsSpokenInSentences() {
        let heard = glance([
            day(0, listenedMs: 4 * Self.hourMs),
            day(1, listenedMs: 3 * Self.hourMs),
        ])

        #expect(heard.spoken.hasPrefix("Last 7 days, 7 hours listened. 1 hour a day."))
        #expect(heard.spoken.contains("The most was Thursday, 4 hours."))
        #expect(heard.spoken.contains("2 days in a row"))
        #expect(heard.spoken.contains("300 hours in all, 120 plays"))
        #expect(heard.spoken.contains("40 of 100 songs heard"))
    }
}
