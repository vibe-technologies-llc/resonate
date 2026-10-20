import Foundation
import ResonateNative
import Testing

@testable import ResonateLogic

func found(_ count: UInt32 = 1, completions: [String] = [], title: String = "Echoes") -> LibrarySearch {
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

actor Asked {
    private(set) var texts: [String] = []

    func note(_ text: String) { texts.append(text) }
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

    @Test func anAnswerToTextTypedPastIsRefusedWhateverOrderTheyLand() async {
        let slow = Asked()
        let search = model { query in
            await slow.note(query)
            if query == "slow" { try await Task.sleep(for: .milliseconds(150)) }
            return found(title: query)
        }

        search.text = "slow"
        try? await Task.sleep(for: .milliseconds(60))
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
        search.text = "echoes"
        await eventually { search.hasAnswer }

        search.refresh()
        await eventually { await asked.texts.count == 2 }

        #expect(await asked.texts == ["echoes", "echoes"])
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

    @Test func completionsOfferedStartWithWhatWasAskedAndAreNeverTheSameWords() async {
        let search = model { _ in found(1, completions: ["Pink Floyd", "pink", "Pinkpop", "Floyd Pink", "PÍNK FLOYD live"]) }

        search.text = "Pink"
        await eventually { search.hasAnswer }

        #expect(search.offeredCompletions == ["Pink Floyd", "Pinkpop", "PÍNK FLOYD live"])
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
        first.text = "echoes"
        first.remember()

        let second = model()

        #expect(second.recents == ["echoes"])
    }

    @Test func aModelNotRememberingKeepsNothingAndNothingIsReadBack() {
        let remembered = model()
        remembered.text = "echoes"
        remembered.remember()

        let forgetful = model(remembering: false)
        forgetful.text = "hunter"
        forgetful.remember()

        #expect(forgetful.recents.isEmpty)
        #expect(model().recents == ["echoes"])
    }

    @Test func switchingHistoryOffHidesAndStopsKeepingRecentSearches() {
        let search = model()
        search.text = "echoes"
        search.remember()
        history.first.set(false, forKey: SearchModel.historyKeptKey)

        search.text = "hunter"
        search.remember()
        search.recallRecents()

        #expect(search.recents.isEmpty)
        #expect(model().recents.isEmpty)
        history.first.set(true, forKey: SearchModel.historyKeptKey)
        #expect(model().recents == ["echoes"])
    }

    @Test func forgettingRecentsEmptiesThemInTheModelAndInStorage() {
        let search = model()
        search.text = "echoes"
        search.remember()

        search.forgetRecents()

        #expect(search.recents.isEmpty)
        #expect(model().recents.isEmpty)
    }

    @Test func recallingRecentsTakesWhatAnotherTabKept() {
        let one = model()
        let other = model()
        other.text = "hunter"
        other.remember()

        one.recallRecents()

        #expect(one.recents == ["hunter"])
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
        #expect(LetterRun.letter(of: "Björk") == "B")
        #expect(LetterRun.letter(of: "Édith Piaf") == "E")
        #expect(LetterRun.letter(of: "élan") == "E")
        #expect(LetterRun.letter(of: "The Wall") == "T")
    }

    @Test func aNameLeadingWithPunctuationIsFiledUnderTheFirstLetterAfterIt() {
        #expect(LetterRun.letter(of: "¡Viva!") == "V")
        #expect(LetterRun.letter(of: "(What's the Story)") == "W")
    }

    @Test func aNumberDigitsAndNamesWithNoLetterFromTheLatinAlphabetFallUnderHash() {
        #expect(LetterRun.letter(of: "") == "#")
        #expect(LetterRun.letter(of: "!!!") == "#")
        #expect(LetterRun.letter(of: "99 Luftballons") == "#")
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
        let text = "Pink Floyd"

        let marked = lightedRanges(in: text, ranges: [TextRange(start: 5, end: 10)])

        #expect(marked.map { String(text[$0]) } == ["Floyd"])
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
        let text = "Bjo\u{0308}rk"

        let marked = lightedRanges(in: text, ranges: [TextRange(start: 2, end: 3)])

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
        let marked = lightedRanges(in: "Pink Floyd", by: "pink", in: .artist)

        #expect(marked.map { String("Pink Floyd"[$0]) } == ["Pink"])
    }
}
