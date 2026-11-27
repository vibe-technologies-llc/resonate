import Foundation
import ResonateNative
import Testing

@testable import ResonateLogic

func found(_ count: UInt32 = 1, completions: [String] = [], title: String = "Slow Water") -> LibrarySearch {
    LibrarySearch(
        tracks: (0..<count).map { track(UInt64($0) + 1, title: title) },
        trackCount: count,
        albums: [],
        artists: [],
        reads: [],
        didYouMean: nil,
        completions: completions
    )
}

actor Favoured {
    private(set) var on = false

    func turnOn() { on = true }
}

actor Asked {
    private(set) var texts: [String] = []

    func note(_ text: String) { texts.append(text) }
}

actor HeldSearch {
    private var waiting: CheckedContinuation<Void, Never>?
    var isWaiting: Bool { waiting != nil }

    func wait() async {
        await withCheckedContinuation { waiting = $0 }
    }

    func release() {
        waiting?.resume()
        waiting = nil
    }
}

final class Reported: @unchecked Sendable {
    private let lock = NSLock()
    private var all: [String] = []

    func note(_ failure: any Swift.Error) { lock.withLock { all.append("\(failure)") } }
    var count: Int { lock.withLock { all.count } }
}

struct Refusal: Swift.Error {}

@MainActor
func eventually(_ condition: @MainActor () async -> Bool) async {
    for _ in 0..<400 {
        if await condition() { return }
        try? await Task.sleep(for: .milliseconds(10))
    }
}

@MainActor @Suite struct SearchModelTests {
    let history = Defaults()
    let asked = Asked()
    let reported = Reported()

    func model(
        remembering: Bool = true,
        ignoringLinks: Bool = false,
        answering: (@Sendable (String) async throws -> LibrarySearch)? = nil
    ) -> SearchModel {
        let asked = asked
        return SearchModel(
            answering: answering ?? { query in
                await asked.note(query)
                return found(title: query)
            },
            reporting: { [reported] failure in reported.note(failure) },
            remembering: remembering,
            ignoringLinks: ignoringLinks,
            historySuite: history.name,
            settle: .milliseconds(20)
        )
    }

    @Test func nothingTypedAsksNothingAndHasNoAnswer() {
        let search = model()

        #expect(!search.isAsked)
        #expect(!search.hasAnswer)
        #expect(!search.matchedNothing)
        #expect(search.found.trackCount == 0)
    }

    @Test func textTypedIsAskedOnceItHasSettledAndAnswered() async {
        let search = model()

        search.text = "  floyd  "
        #expect(search.asking)
        await eventually { search.hasAnswer }

        #expect(await asked.texts == ["floyd"])
        #expect(search.answered == "floyd")
        #expect(search.found.tracks.first?.title == "floyd")
        #expect(!search.asking)
    }

    @Test func keystrokesInsideTheSettleAskForTheLastTextOnly() async {
        let search = model()

        search.text = "f"
        search.text = "fl"
        search.text = "flo"
        await eventually { search.hasAnswer }

        #expect(await asked.texts == ["flo"])
    }

    @Test func onlyTheLatestQueryRunsAfterAnUncancellableLookup() async {
        let held = HeldSearch()
        let asked = asked
        let search = model { query in
            await asked.note(query)
            if query == "f" { await held.wait() }
            return found(title: query)
        }

        search.text = "f"
        await eventually { await held.isWaiting }
        search.text = "fl"
        search.refresh()
        await Task.yield()
        search.text = "flo"
        try? await Task.sleep(for: .milliseconds(80))
        #expect(await asked.texts == ["f"])

        await held.release()
        await eventually { search.answered == "flo" }

        #expect(await asked.texts == ["f", "flo"])
        #expect(search.found.tracks.first?.title == "flo")
    }

    @Test func hiddenLibraryResultsAndLinksAskNothing() async {
        let search = model(ignoringLinks: true)
        search.text = "https://example.test/album"
        #expect(!search.asking)
        search.includesLibrary = false
        search.text = "floyd"
        #expect(!search.asking)
        #expect(await asked.texts.isEmpty)

        search.includesLibrary = true
        await eventually { search.answered == "floyd" }
        search.text = "  floyd "
        #expect(!search.asking)
        #expect(await asked.texts == ["floyd"])
    }

    @Test func aRefreshThatOnlyReranksTheSameSongsKeepsTheOrderShown() async {
        let favoured = Favoured()
        let search = model { _ in
            let ranked = await favoured.on
                ? [track(2, title: "Two", favourite: true), track(1, title: "One"), track(3, title: "Three")]
                : [track(1, title: "One"), track(2, title: "Two"), track(3, title: "Three")]
            return LibrarySearch(tracks: ranked, trackCount: 3, albums: [], artists: [], reads: [], didYouMean: nil, completions: [])
        }

        search.text = "song"
        await eventually { search.hasAnswer }
        await favoured.turnOn()
        search.refresh()
        await eventually { search.found.tracks.contains { $0.favourite } }

        #expect(search.found.tracks.map(\.id) == [1, 2, 3])
        #expect(search.found.tracks[1].favourite)
    }

    @Test func aRefreshThatChangesWhichSongsMatchShowsTheNewOrder() {
        let shown = LibrarySearch(tracks: [track(1), track(2)], trackCount: 2, albums: [], artists: [], reads: [], didYouMean: nil, completions: [])
        let answer = LibrarySearch(tracks: [track(3), track(1)], trackCount: 2, albums: [], artists: [], reads: [], didYouMean: nil, completions: [])

        #expect(SearchModel.steadied(answer, as: shown).tracks.map(\.id) == [3, 1])
    }

    @Test func anAnswerToTextTypedPastIsRefusedWhateverOrderTheyLand() async {
        let slow = Asked()
        let search = model { query in
            await slow.note(query)
            if query == "slow" { try await Task.sleep(for: .milliseconds(150)) }
            return found(title: query)
        }

        search.text = "slow"
        await eventually { await slow.texts == ["slow"] }
        search.text = "quick"
        await eventually { search.answered == "quick" }
        try? await Task.sleep(for: .milliseconds(200))

        #expect(await slow.texts == ["slow", "quick"])
        #expect(search.answered == "quick")
        #expect(search.found.tracks.first?.title == "quick")
        #expect(!search.asking)
    }

    @Test func clearingTheTextDropsTheAnswerAndAnyLateOne() async {
        let search = model { query in
            try await Task.sleep(for: .milliseconds(100))
            return found(title: query)
        }
        search.text = "slow"
        try? await Task.sleep(for: .milliseconds(60))

        search.text = ""
        try? await Task.sleep(for: .milliseconds(200))

        #expect(!search.hasAnswer)
        #expect(search.found.trackCount == 0)
        #expect(!search.asking)
    }

    @Test func textOfOnlySpacesIsNotAsked() async {
        let search = model()

        search.text = "   "
        try? await Task.sleep(for: .milliseconds(80))

        #expect(!search.isAsked)
        #expect(await asked.texts.isEmpty)
    }

    @Test func aFailureForTheCurrentTextIsReportedAndKeepsTheLastAnswerOnScreen() async {
        let search = model { query in
            if query == "bad" { throw Refusal() }
            return found(title: query)
        }
        search.text = "good"
        await eventually { search.answered == "good" }

        search.text = "bad"
        await eventually { reported.count == 1 }

        #expect(reported.count == 1)
        #expect(search.answered == "good")
    }

    @Test func aFailureForTextTypedPastIsNotReported() async {
        let search = model { query in
            if query == "bad" {
                try await Task.sleep(for: .milliseconds(100))
                throw Refusal()
            }
            return found(title: query)
        }
        search.text = "bad"
        try? await Task.sleep(for: .milliseconds(60))
        search.text = "good"
        await eventually { search.answered == "good" }
        try? await Task.sleep(for: .milliseconds(200))

        #expect(reported.count == 0)
        #expect(search.answered == "good")
    }

    @Test func refreshAsksAgainAtOnceWithoutTheSettle() async {
        let search = model()
        search.text = "slow water"
        await eventually { search.hasAnswer }

        search.refresh()
        await eventually { await asked.texts.count == 2 }

        #expect(await asked.texts == ["slow water", "slow water"])
    }

    @Test func refreshWithNothingTypedAsksNothing() async {
        let search = model()

        search.refresh()
        try? await Task.sleep(for: .milliseconds(60))

        #expect(await asked.texts.isEmpty)
    }

    @Test func aSearchThatFoundNothingSaysSo() async {
        let search = model { _ in found(0) }

        search.text = "zzz"
        await eventually { search.hasAnswer }

        #expect(search.matchedNothing)
    }

    @Test func recentSearchesAreKeptNewestFirstWithoutRepeatsInAnyCaseAndTenAtMost() {
        let search = model()

        for word in ["a", "b", "c", "A", "d", "e", "f", "g", "h", "i", "j", "k"] {
            search.text = word
            search.remember()
        }

        #expect(search.recents == ["k", "j", "i", "h", "g", "f", "e", "d", "A", "c"])
    }

    @Test func aSearchTypedBlankIsNotRemembered() {
        let search = model()

        search.text = "   "
        search.remember()

        #expect(search.recents.isEmpty)
    }

    @Test func recentSearchesSurviveToAnotherModelOnTheSameDefaults() {
        let first = model()
        first.text = "slow water"
        first.remember()

        let second = model()

        #expect(second.recents == ["slow water"])
    }

    @Test func aModelNotRememberingKeepsNothingAndNothingIsReadBack() {
        let remembered = model()
        remembered.text = "slow water"
        remembered.remember()

        let forgetful = model(remembering: false)
        forgetful.text = "wavebreak"
        forgetful.remember()

        #expect(forgetful.recents.isEmpty)
        #expect(model().recents == ["slow water"])
    }

    @Test func switchingHistoryOffHidesAndStopsKeepingRecentSearches() {
        let search = model()
        search.text = "slow water"
        search.remember()
        history.first.set(false, forKey: SearchModel.historyKeptKey)

        search.text = "wavebreak"
        search.remember()
        search.recallRecents()

        #expect(search.recents.isEmpty)
        #expect(model().recents.isEmpty)
        history.first.set(true, forKey: SearchModel.historyKeptKey)
        #expect(model().recents == ["slow water"])
    }

    @Test func forgettingRecentsEmptiesThemInTheModelAndInStorage() {
        let search = model()
        search.text = "slow water"
        search.remember()

        search.forgetRecents()

        #expect(search.recents.isEmpty)
        #expect(model().recents.isEmpty)
    }

    @Test func recallingRecentsTakesWhatAnotherTabKept() {
        let one = model()
        let other = model()
        other.text = "wavebreak"
        other.remember()

        one.recallRecents()

        #expect(one.recents == ["wavebreak"])
    }
}

@Suite struct LetterRunTests {
    func rows(_ names: [String]) -> [String] { names }

    @Test func aShortListHasNoRunsAndNoIndex() {
        let few = (0..<39).map { _ in "Abba" }

        #expect(LetterRun.runs(of: few, named: { $0 }).isEmpty)
    }

    @Test func aLongListOfOneLetterIsNotWorthAnIndex() {
        let one = (0..<60).map { _ in "Abba" }

        #expect(LetterRun.runs(of: one, named: { $0 }).isEmpty)
    }

    @Test func aLongListIsCutIntoRunsOfEachLetterInOrder() {
        let names = (0..<20).map { _ in "Abba" } + (0..<20).map { _ in "Beck" } + (0..<5).map { _ in "Cher" }

        let runs = LetterRun.runs(of: names, named: { $0 })

        #expect(runs.map(\.letter) == ["A", "B", "C"])
        #expect(runs.map(\.rows) == [0..<20, 20..<40, 40..<45])
        #expect(runs.allSatisfy { $0.indexed })
    }

    @Test func aLetterComingBackIsARunOfItsOwnAndOnlyTheFirstIsIndexed() {
        let names = (0..<20).map { _ in "Abba" } + (0..<20).map { _ in "Beck" } + (0..<5).map { _ in "Abel" }

        let runs = LetterRun.runs(of: names, named: { $0 })

        #expect(runs.map(\.letter) == ["A", "B", "A"])
        #expect(runs.map(\.indexed) == [true, true, false])
        #expect(Set(runs.map(\.id)).count == 3)
    }

    @Test func aNameIsFiledUnderItsFirstLetterOrNumberFoldedOfAccents() {
        #expect(LetterRun.letter(of: "Sölvi") == "S")
        #expect(LetterRun.letter(of: "Édith Moreau") == "E")
        #expect(LetterRun.letter(of: "élan") == "E")
        #expect(LetterRun.letter(of: "The Pier") == "T")
    }

    @Test func aNameLeadingWithPunctuationIsFiledUnderTheFirstLetterAfterIt() {
        #expect(LetterRun.letter(of: "¡Viva!") == "V")
        #expect(LetterRun.letter(of: "(What's the Story)") == "W")
    }

    @Test func aNumberDigitsAndNamesWithNoLetterFromTheLatinAlphabetFallUnderHash() {
        #expect(LetterRun.letter(of: "") == "#")
        #expect(LetterRun.letter(of: "!!!") == "#")
        #expect(LetterRun.letter(of: "99 Lanterns") == "#")
        #expect(LetterRun.letter(of: "Ελλάδα") == "#")
        #expect(LetterRun.letter(of: "日本") == "#")
    }

    @Test func clampingRowsKeepsThemInsideTheCollection() {
        let rows = [1, 2, 3, 4, 5]

        #expect(rows.clamped(2..<9) == 2..<5)
        #expect(rows.clamped(0..<3) == 0..<3)
        #expect(rows.clamped(7..<9) == 5..<5)
    }
}

@Suite struct LightedRangesTests {
    @Test func aRangeInUtf16UnitsMarksTheSameCharactersInTheString() {
        let text = "Example Band"

        let marked = lightedRanges(in: text, ranges: [TextRange(start: 8, end: 12)])

        #expect(marked.map { String(text[$0]) } == ["Band"])
    }

    @Test func aRangeCountsBothHalvesOfASurrogatePairAsTwoUnits() {
        let text = "a😀b"

        let marked = lightedRanges(in: text, ranges: [TextRange(start: 1, end: 3)])

        #expect(marked.map { String(text[$0]) } == ["😀"])
    }

    @Test func aRangeCuttingACharacterInHalfIsWidenedToTheWholeCharacter() {
        let text = "a😀b"

        let marked = lightedRanges(in: text, ranges: [TextRange(start: 1, end: 2)])

        #expect(marked.map { String(text[$0]) } == ["😀"])
    }

    @Test func aRangeInsideAnAccentedLetterMarksTheWholeLetterWithItsAccent() {
        let text = "So\u{0308}lvi"

        let marked = lightedRanges(in: text, ranges: [TextRange(start: 1, end: 2)])

        #expect(marked.map { String(text[$0]) } == ["o\u{0308}"])
    }

    @Test func aRangePastTheEndIsHeldToTheStringAndAnEmptyOrBackwardsOneIsDropped() {
        let text = "abc"

        let marked = lightedRanges(
            in: text,
            ranges: [TextRange(start: 1, end: 99), TextRange(start: 2, end: 2), TextRange(start: 9, end: 20), TextRange(start: 3, end: 1)]
        )

        #expect(marked.map { String(text[$0]) } == ["bc"])
    }

    @Test func noRangesMarkNothing() {
        #expect(lightedRanges(in: "abc", ranges: []).isEmpty)
        #expect(lightedRanges(in: "abc", by: "", in: .title).isEmpty)
    }

    @Test func aQueryThatMatchesTheTitleMarksWhereTheEngineSaysItDoes() {
        let marked = lightedRanges(in: "Example Band", by: "exam", in: .artist)

        #expect(marked.map { String("Example Band"[$0]) } == ["Exam"])
    }
}
