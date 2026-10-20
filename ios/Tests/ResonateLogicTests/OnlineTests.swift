import Foundation
import ResonateNative
import Testing

@testable import ResonateLogic

func onlineAlbum(_ title: String = "Meddle") -> SearchAlbum {
    SearchAlbum(id: "id-\(title)", title: title, artist: "Pink Floyd", date: "1971")
}

func onlineArtist(_ name: String = "Pink Floyd") -> SearchArtist {
    SearchArtist(id: "id-\(name)", name: name, kind: "Group")
}

@Suite struct OnlineAskedTests {
    @Test func aLinkIsOneRequestWithNoMode() {
        #expect(OnlineAsked.chain("https://open.spotify.com/track/abc") == [OnlineAsked(text: "https://open.spotify.com/track/abc", mode: nil)])
        #expect(OnlineAsked.chain("spotify:track:abc").count == 1)
    }

    @Test func wordsAreAChainOfSongsThenAlbumsThenArtists() {
        #expect(
            OnlineAsked.chain("echoes") == [
                OnlineAsked(text: "echoes", mode: .songs),
                OnlineAsked(text: "echoes", mode: .albums),
                OnlineAsked(text: "echoes", mode: .artists),
            ]
        )
    }

    @Test func nothingIsAskedForNoText() {
        #expect(OnlineAsked.chain("").isEmpty)
    }

    @Test(arguments: ["http://x", "HTTPS://x", "Spotify:album:1"])
    func aLinkIsKnownByItsSchemeInAnyCase(text: String) {
        #expect(OnlineAsked.isLink(text))
    }

    @Test(arguments: ["ftp://x", "spotify", "httpx://", "a http://x", ""])
    func aTextThatOnlyLooksLikeOneIsNot(text: String) {
        #expect(!OnlineAsked.isLink(text))
    }

    @Test func twoShortTextsAreAskedAtOnceAsTheyAreTypedWithNoFloor() {
        #expect(OnlineAsked.chain("U2").count == 3)
        #expect(OnlineAsked.chain("日本").count == 3)
    }
}

@Suite struct OnlineFoundTests {
    @Test func nothingFoundIsNotWholeAndAwaitsSongsFirst() {
        let found = OnlineFound()

        #expect(!found.isWhole)
        #expect(found.isEmpty)
        #expect(found.awaited(for: "echoes") == .songs)
    }

    @Test func eachAnswerLandsInItsSectionAndTheNextAwaitedMovesOn() {
        var found = OnlineFound().landing(.songs([searchSong()]), for: "echoes")
        #expect(found.awaited(for: "echoes") == .albums)

        found = found.landing(.albums([onlineAlbum()]), for: "echoes")
        #expect(found.awaited(for: "echoes") == .artists)
        #expect(!found.isWhole)

        found = found.landing(.artists([onlineArtist()]), for: "echoes")
        #expect(found.awaited(for: "echoes") == nil)
        #expect(found.isWhole)
        #expect(!found.isEmpty)
    }

    @Test func anAnswerForNewTextStartsFreshAndDropsTheOldSpellingsSections() {
        let before = OnlineFound().landing(.songs([searchSong()]), for: "echo").landing(.albums([onlineAlbum()]), for: "echo")

        let after = before.landing(.songs([searchSong("Hunter")]), for: "hunter")

        #expect(after.text == "hunter")
        #expect(after.songs?.first?.title == "Hunter")
        #expect(after.albums == nil)
        #expect(after.awaited(for: "hunter") == .albums)
    }

    @Test func theSectionsOfOldTextStayUntilNewTextsFirstAnswerLands() {
        let before = OnlineFound().landing(.songs([searchSong()]), for: "echo")

        #expect(before.awaited(for: "echoes") == .songs)
        #expect(before.songs?.first?.title == "Echoes")
    }

    @Test func aLinkIsWholeByItself() {
        let collection = OnlineCollection(name: "Playlist", songs: [searchSong()], releases: [], artists: [])

        let found = OnlineFound().landing(.link(collection), for: "https://x")

        #expect(found.isWhole)
        #expect(!found.isEmpty)
    }

    @Test func aWholeAnswerWithNothingInAnySectionIsEmpty() {
        let found = OnlineFound(text: "zzz", answers: [.songs([]), .albums([]), .artists([])])

        #expect(found.isWhole)
        #expect(found.isEmpty)
    }

    @Test func aLinkWithNothingInItIsEmpty() {
        let found = OnlineFound(text: "x", answers: [.link(OnlineCollection(name: "", songs: [], releases: [], artists: []))])

        #expect(found.isEmpty)
    }
}

@Suite struct OnlineAnswersTests {
    func asked(_ text: String, _ mode: OnlineMode? = .songs) -> OnlineAsked {
        OnlineAsked(text: text, mode: mode)
    }

    @Test func anAnswerKeptIsFoundAgainBySpelling() {
        var answers = OnlineAnswers()

        answers.keep(.songs([searchSong()]), for: asked("echoes"))

        #expect(answers.answer(for: asked("echoes")) != nil)
        #expect(answers.answer(for: asked("echoes", .albums)) == nil)
        #expect(answers.answer(for: asked("echo")) == nil)
    }

    @Test func theSixtyFifthSpellingOfAModeForgetsTheOldestOfThatMode() {
        var answers = OnlineAnswers()

        for number in 0..<65 { answers.keep(.songs([]), for: asked("spelling \(number)")) }

        #expect(answers.answer(for: asked("spelling 0")) == nil)
        #expect(answers.answer(for: asked("spelling 1")) != nil)
        #expect(answers.answer(for: asked("spelling 64")) != nil)
    }

    @Test func aSpellingAskedAgainIsTheNewestAndOutlastsOlderOnes() {
        var answers = OnlineAnswers()
        for number in 0..<64 { answers.keep(.songs([]), for: asked("spelling \(number)")) }

        answers.keep(.songs([]), for: asked("spelling 0"))
        answers.keep(.songs([]), for: asked("spelling 64"))

        #expect(answers.answer(for: asked("spelling 0")) != nil)
        #expect(answers.answer(for: asked("spelling 1")) == nil)
    }

    @Test func eachModeKeepsItsOwnSixtyFour() {
        var answers = OnlineAnswers()
        for number in 0..<64 { answers.keep(.songs([]), for: asked("s\(number)", .songs)) }

        for number in 0..<64 { answers.keep(.albums([]), for: asked("a\(number)", .albums)) }

        #expect(answers.answer(for: asked("s0", .songs)) != nil)
        #expect(answers.answer(for: asked("a0", .albums)) != nil)
    }

    @Test func aLinkIsKeptUnderNoMode() {
        var answers = OnlineAnswers()

        answers.keep(.link(OnlineCollection(name: "x", songs: [], releases: [], artists: [])), for: asked("https://x", nil))

        #expect(answers.answer(for: asked("https://x", nil)) != nil)
    }
}

@Suite struct ListeningBarTests {
    var calendar: Calendar {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = TimeZone(secondsFromGMT: 0)!
        calendar.firstWeekday = 2
        return calendar
    }

    func day(_ iso: String, plays: UInt64 = 1, listened: UInt64 = 60_000) -> ListenedOnDay {
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withFullDate]
        formatter.timeZone = TimeZone(secondsFromGMT: 0)
        return ListenedOnDay(starts: UInt64(formatter.date(from: iso)!.timeIntervalSince1970), plays: plays, listenedMs: listened)
    }

    @Test func aWeekOrAMonthIsOneBarADay() {
        let bars = ListeningBar.grouped([day("2026-03-02"), day("2026-03-03", plays: 2, listened: 5_000)], span: 7, calendar: calendar)

        #expect(bars.count == 2)
        #expect(bars.map(\.plays) == [1, 2])
        #expect(bars.map(\.listenedMs) == [60_000, 5_000])
    }

    @Test func aYearIsOneBarAWeekSummingItsDays() {
        let days = [day("2026-03-02", plays: 1, listened: 10), day("2026-03-04", plays: 2, listened: 20), day("2026-03-09", plays: 4, listened: 40)]

        let bars = ListeningBar.grouped(days, span: 365, calendar: calendar)

        #expect(bars.count == 2)
        #expect(bars.map(\.plays) == [3, 4])
        #expect(bars.map(\.listenedMs) == [30, 40])
    }

    @Test func allTimeIsOneBarAMonthSummingItsDays() {
        let days = [day("2026-01-31", plays: 1, listened: 10), day("2026-02-01", plays: 2, listened: 20), day("2026-02-28", plays: 4, listened: 40)]

        let bars = ListeningBar.grouped(days, span: 0, calendar: calendar)

        #expect(bars.map(\.plays) == [1, 6])
        #expect(bars.map(\.listenedMs) == [10, 60])
    }

    @Test func barsComeOldestFirstWhateverOrderTheDaysCameIn() {
        let bars = ListeningBar.grouped([day("2026-03-05"), day("2026-03-01"), day("2026-03-03")], span: 30, calendar: calendar)

        #expect(bars.map(\.starts) == bars.map(\.starts).sorted())
        #expect(bars.count == 3)
    }

    @Test func nothingHeardMakesNoBars() {
        #expect(ListeningBar.grouped([], span: 7, calendar: calendar).isEmpty)
    }

    @Test func aBarIsKeyedByWhenItStarts() {
        let bar = ListeningBar.grouped([day("2026-03-02")], span: 7, calendar: calendar)[0]

        #expect(bar.id == bar.starts)
    }

    @Test func theUnitFollowsTheSpan() {
        #expect(ListeningBar.unit(for: 0) == .month)
        #expect(ListeningBar.unit(for: 365) == .weekOfYear)
        #expect(ListeningBar.unit(for: 7) == .day)
        #expect(ListeningBar.unit(for: 30) == .day)
    }
}
