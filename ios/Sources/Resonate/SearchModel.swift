import Foundation
import Observation
import ResonateNative
import SwiftUI

enum SearchScope: String, CaseIterable, Identifiable {
    case top = "Top"
    case songs = "Songs"
    case albums = "Albums"
    case artists = "Artists"

    var id: String { rawValue }
}

@MainActor
@Observable
final class SearchModel {
    nonisolated static let settle: Duration = .milliseconds(150)
    nonisolated static let mostSongs: UInt32 = 500
    private static let recentsKept = 10
    private static let recentsKey = "recent-searches"
    private static let nothing = LibrarySearch(
        tracks: [],
        trackCount: 0,
        albums: [],
        artists: [],
        reads: [],
        didYouMean: nil,
        completions: []
    )

    var text = "" {
        didSet { if text != oldValue { ask() } }
    }
    private(set) var found = SearchModel.nothing
    private(set) var answered = ""
    private(set) var asking = false
    private(set) var recents: [String] = []

    @ObservationIgnored private unowned let library: Library
    @ObservationIgnored private let remembered: UserDefaults?
    @ObservationIgnored private var pending: Task<Void, Never>?
    @ObservationIgnored private var generation = 0

    init(library: Library, remembering: Bool) {
        self.library = library
        remembered = remembering ? UserDefaults(suiteName: "library-features") : nil
        recents = remembered?.stringArray(forKey: Self.recentsKey) ?? []
    }

    var query: String { text.trimmingCharacters(in: .whitespacesAndNewlines) }
    var isAsked: Bool { !query.isEmpty }
    var hasAnswer: Bool { !answered.isEmpty }
    var matchedNothing: Bool { hasAnswer && found.trackCount == 0 }

    var offeredCompletions: [String] {
        let typed = answered.searchFolded
        return found.completions.filter { completion in
            let folded = completion.searchFolded
            return folded != typed && folded.hasPrefix(typed)
        }
    }

    func refresh() {
        if isAsked { ask(settling: false) }
    }

    func remember() {
        guard let remembered else { return }
        let kept = query
        guard !kept.isEmpty else { return }

        let stored = remembered.stringArray(forKey: Self.recentsKey) ?? []
        recents = Array(([kept] + stored.filter { $0.caseInsensitiveCompare(kept) != .orderedSame }).prefix(Self.recentsKept))
        remembered.set(recents, forKey: Self.recentsKey)
    }

    func recallRecents() {
        let stored = remembered?.stringArray(forKey: Self.recentsKey) ?? []
        if stored != recents { recents = stored }
    }

    func forgetRecents() {
        recents = []
        remembered?.removeObject(forKey: Self.recentsKey)
    }

    private func ask(settling: Bool = true) {
        generation += 1
        pending?.cancel()
        let asked = generation
        let query = query

        guard !query.isEmpty else {
            found = Self.nothing
            answered = ""
            asking = false
            pending = nil
            return
        }

        asking = true
        pending = Task { [weak self] in
            if settling {
                do { try await Task.sleep(for: SearchModel.settle) } catch { return }
            }
            guard let self else { return }

            do {
                let native = try await library.player()
                let answer = try await Work.aside { try native.searchLibrary(text: query, limit: SearchModel.mostSongs) }
                guard asked == generation else { return }
                found = answer
                answered = query
            } catch {
                guard asked == generation else { return }
                library.report(error)
            }
            asking = false
        }
    }
}

extension String {
    var searchFolded: String {
        foldedText(text: self)
    }
}

func lit(_ text: String, by query: String, in column: SearchColumn) -> AttributedString {
    var shown = AttributedString(text)
    let ranges = query.isEmpty ? [] : highlights(query: query, text: text, column: column)
    guard !ranges.isEmpty else { return shown }

    shown.foregroundColor = Color.secondary
    let units = text.utf16.count
    for range in ranges {
        let start = min(Int(range.start), units)
        let end = min(Int(range.end), units)
        guard start < end, let marked = Range(NSRange(location: start, length: end - start), in: text) else { continue }
        let whole = text.rangeOfComposedCharacterSequences(for: marked)
        guard let shownRange = Range(whole, in: shown) else { continue }
        shown[shownRange].foregroundColor = Color.primary
        shown[shownRange].inlinePresentationIntent = .stronglyEmphasized
    }
    return shown
}

struct LetterRun: Identifiable {
    static let indexedFrom = 40

    let id: Int
    let letter: String
    let rows: Range<Int>
    let indexed: Bool

    static func runs<Row>(of rows: [Row], named name: (Row) -> String) -> [LetterRun] {
        guard rows.count >= indexedFrom else { return [] }

        var runs: [LetterRun] = []
        var used = Set<String>()
        var start = 0
        var current = letter(of: name(rows[0]))
        for index in 1...rows.count {
            let next = index < rows.count ? letter(of: name(rows[index])) : nil
            guard next != current else { continue }

            runs.append(LetterRun(id: runs.count, letter: current, rows: start..<index, indexed: used.insert(current).inserted))
            start = index
            if let next { current = next }
        }
        return runs.count > 1 ? runs : []
    }

    static func letter(of name: String) -> String {
        guard let first = name.first(where: { $0.isLetter || $0.isNumber }) else { return "#" }
        if first.isASCII, first.isLetter { return first.uppercased() }

        let folded = String(first).searchFolded.uppercased()
        guard let plain = folded.first, plain.isASCII, plain.isLetter else { return "#" }
        return String(plain)
    }
}

extension Collection where Index == Int {
    func clamped(_ rows: Range<Int>) -> Range<Int> {
        rows.clamped(to: startIndex..<endIndex)
    }
}
