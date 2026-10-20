import Foundation
import ResonateLogic
import ResonateNative
import SwiftUI

enum SearchScope: String, CaseIterable, Identifiable {
    case top = "Top"
    case songs = "Songs"
    case albums = "Albums"
    case artists = "Artists"

    var id: String { rawValue }
}

private struct LitAsked: Hashable {
    let text: String
    let query: String
    let column: SearchColumn
}

private let litKeptAtMost = 4096
@MainActor private var litKept: [LitAsked: AttributedString] = [:]

@MainActor
func lit(_ text: String, by query: String, in column: SearchColumn) -> AttributedString {
    let asked = LitAsked(text: text, query: query, column: column)
    if let kept = litKept[asked] { return kept }
    let shown = lighted(text, by: query, in: column)
    if litKept.count >= litKeptAtMost { litKept.removeAll(keepingCapacity: true) }
    litKept[asked] = shown
    return shown
}

private func lighted(_ text: String, by query: String, in column: SearchColumn) -> AttributedString {
    var shown = AttributedString(text)
    let marked = lightedRanges(in: text, by: query, in: column)
    guard !marked.isEmpty else { return shown }

    shown.foregroundColor = Color.secondary
    for whole in marked {
        guard let shownRange = Range(whole, in: shown) else { continue }
        shown[shownRange].foregroundColor = Color.primary
        shown[shownRange].inlinePresentationIntent = .stronglyEmphasized
    }
    return shown
}

extension SearchModel {
    convenience init(library: Library, remembering: Bool) {
        self.init(
            answering: { [unowned library] query in
                let native = try await library.player()
                return try await Work.aside { try native.searchLibrary(text: query, limit: SearchModel.mostSongs) }
            },
            reporting: { [unowned library] error in library.report(error) },
            remembering: remembering
        )
    }
}
