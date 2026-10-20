import Foundation
import Observation
import ResonateNative

@MainActor
@Observable
public final class SearchModel {
    public nonisolated static let settle: Duration = .milliseconds(150)
    public nonisolated static let mostSongs: UInt32 = 500
    private static let recentsKept = 10
    private static let recentsKey = "recent-searches"
    public static let historyKeptKey = "keep-search-history"
    public static let historySuite = "library-features"
    private static let nothing = LibrarySearch(
        tracks: [],
        trackCount: 0,
        albums: [],
        artists: [],
        reads: [],
        didYouMean: nil,
        completions: []
    )

    public var text = "" {
        didSet { if text != oldValue { ask() } }
    }
    public private(set) var found = SearchModel.nothing
    public private(set) var answered = ""
    public private(set) var asking = false
    public private(set) var recents: [String] = []

    @ObservationIgnored private let answering: @Sendable (String) async throws -> LibrarySearch
    @ObservationIgnored private let reporting: @MainActor (any Swift.Error) -> Void
    @ObservationIgnored private let settle: Duration
    @ObservationIgnored private let remembered: UserDefaults?
    @ObservationIgnored private var pending: Task<Void, Never>?
    @ObservationIgnored private var generation = 0

    public init(
        answering asking: @escaping @Sendable (String) async throws -> LibrarySearch,
        reporting: @escaping @MainActor (any Swift.Error) -> Void,
        remembering: Bool,
        historySuite: String = SearchModel.historySuite,
        settle: Duration = SearchModel.settle
    ) {
        self.answering = asking
        self.reporting = reporting
        self.settle = settle
        remembered = remembering ? UserDefaults(suiteName: historySuite) : nil
        recents = Self.kept(remembered) ? remembered?.stringArray(forKey: Self.recentsKey) ?? [] : []
    }

    private static func kept(_ remembered: UserDefaults?) -> Bool {
        remembered?.object(forKey: historyKeptKey) as? Bool ?? true
    }

    public var query: String { text.trimmingCharacters(in: .whitespacesAndNewlines) }
    public var isAsked: Bool { !query.isEmpty }
    public var hasAnswer: Bool { !answered.isEmpty }
    public var matchedNothing: Bool { hasAnswer && found.trackCount == 0 }

    public var offeredCompletions: [String] {
        let typed = answered.searchFolded
        return found.completions.filter { completion in
            let folded = completion.searchFolded
            return folded != typed && folded.hasPrefix(typed)
        }
    }

    public func refresh() {
        if isAsked { ask(settling: false) }
    }

    public func remember() {
        guard let remembered, Self.kept(remembered) else { return }
        let kept = query
        guard !kept.isEmpty else { return }

        let stored = remembered.stringArray(forKey: Self.recentsKey) ?? []
        recents = Array(([kept] + stored.filter { $0.caseInsensitiveCompare(kept) != .orderedSame }).prefix(Self.recentsKept))
        remembered.set(recents, forKey: Self.recentsKey)
    }

    public func recallRecents() {
        let stored = Self.kept(remembered) ? remembered?.stringArray(forKey: Self.recentsKey) ?? [] : []
        if stored != recents { recents = stored }
    }

    public func forgetRecents() {
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
        let settle = settle
        pending = Task { [weak self] in
            if settling {
                do { try await Task.sleep(for: settle) } catch { return }
            }
            guard let self else { return }

            do {
                let answer = try await answering(query)
                guard asked == generation else { return }
                found = answer
                answered = query
            } catch {
                guard asked == generation else { return }
                reporting(error)
            }
            asking = false
        }
    }
}

extension String {
    public var searchFolded: String {
        foldedText(text: self)
    }
}

public func lightedRanges(in text: String, by query: String, in column: SearchColumn) -> [Range<String.Index>] {
    lightedRanges(in: text, ranges: query.isEmpty ? [] : highlights(query: query, text: text, column: column))
}

public func lightedRanges(in text: String, ranges: [TextRange]) -> [Range<String.Index>] {
    let units = text.utf16.count
    var marked: [Range<String.Index>] = []
    for range in ranges {
        let start = min(Int(range.start), units)
        let end = min(Int(range.end), units)
        guard start < end else { continue }

        var first: String.Index?
        var last: String.Index?
        var offset = 0
        var index = text.startIndex
        while index < text.endIndex, offset < end {
            let next = text.index(after: index)
            let width = text.utf16.distance(from: index, to: next)
            if offset + width > start {
                first = first ?? index
                last = next
            }
            offset += width
            index = next
        }
        if let first, let last { marked.append(first..<last) }
    }
    return marked
}

public struct LetterRun: Identifiable, Equatable, Sendable {
    public static let indexedFrom = 40

    public let id: String
    public let letter: String
    public let rows: Range<Int>
    public let indexed: Bool

    public static func runs<Row>(of rows: [Row], named name: (Row) -> String) -> [LetterRun] {
        guard rows.count >= indexedFrom else { return [] }

        var runs: [LetterRun] = []
        var seen: [String: Int] = [:]
        var start = 0
        var current = letter(of: name(rows[0]))
        for index in 1...rows.count {
            let next = index < rows.count ? letter(of: name(rows[index])) : nil
            guard next != current else { continue }

            let before = seen[current, default: 0]
            seen[current] = before + 1
            runs.append(LetterRun(id: "\(current) \(before)", letter: current, rows: start..<index, indexed: before == 0))
            start = index
            if let next { current = next }
        }
        return runs.count > 1 ? runs : []
    }

    public static func letter(of name: String) -> String {
        guard let first = name.first(where: { $0.isLetter || $0.isNumber }) else { return "#" }
        if first.isASCII, first.isLetter { return first.uppercased() }

        let folded = String(first).searchFolded.uppercased()
        guard let plain = folded.first, plain.isASCII, plain.isLetter else { return "#" }
        return String(plain)
    }
}

extension Collection where Index == Int {
    public func clamped(_ rows: Range<Int>) -> Range<Int> {
        rows.clamped(to: startIndex..<endIndex)
    }
}
