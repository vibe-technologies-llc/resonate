import ResonateLogic
import ResonateNative
import SwiftUI

enum HigherVersionsShown {
    case reading
    case found(HigherVersions)
    case failed(String)
}

struct HigherVersionsSheet: View {
    @Environment(Library.self) private var library
    @Environment(\.dismiss) private var dismiss
    let id: UInt64
    let name: String
    @State private var shown = HigherVersionsShown.reading
    @State private var ticked: Set<UInt64> = []
    @State private var asked = 0
    @State private var keeping = false

    var body: some View {
        NavigationStack {
            List {
                if case .found(let found) = shown, let trouble = found.trouble {
                    Label(trouble, systemImage: "exclamationmark.triangle").foregroundStyle(.secondary)
                }
                Section {
                    content
                } footer: {
                    Text("higher.detail")
                }
            }
            .navigationTitle(said("higher.title", name))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button(role: .close) { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("higher.keep") { keep() }
                        .disabled(ticked.isEmpty || keeping)
                }
            }
        }
        .task(id: asked) { await read() }
    }

    private var songs: [HigherVersion] {
        if case .found(let found) = shown { return found.songs }
        return []
    }

    @ViewBuilder
    private var content: some View {
        switch shown {
        case .reading:
            ProgressView().accessibilityLabel("higher.reading").frame(maxWidth: .infinity)
        case .found(let found) where found.songs.isEmpty:
            ContentUnavailableView("higher.nothing", systemImage: "checkmark.circle", description: Text("higher.nothing.detail"))
                .listRowSeparator(.hidden)
        case .found(let found):
            ForEach(found.songs, id: \.song.id) { higher in
                HigherVersionRow(higher: higher, ticked: ticked.contains(higher.song.id)) {
                    if ticked.contains(higher.song.id) {
                        ticked.remove(higher.song.id)
                    } else {
                        ticked.insert(higher.song.id)
                    }
                }
                .disabled(keeping || !HigherVersionRows.tickable(higher))
            }
        case .failed(let failure):
            ReadFailure(text: failure) { asked += 1 }
        }
    }

    private func read() async {
        do {
            let found = try await library.features.higherVersionsOf(id)
            if case .found = shown {
                ticked = HigherVersionRows.stillTicked(found.songs, ticked)
            } else {
                ticked = HigherVersionRows.ticked(found.songs)
            }
            shown = .found(found)
        } catch {
            shown = .failed(explained(error))
        }
    }

    private func keep() {
        let picked = HigherVersionRows.picked(songs, ticked)
        keeping = true
        Task {
            await library.features.keepHigherVersions(picked)
            keeping = false
            asked += 1
        }
    }
}

private struct HigherVersionRow: View {
    @ScaledMetric(relativeTo: .body) private var artworkEdge: CGFloat = 44
    let higher: HigherVersion
    let ticked: Bool
    let tick: () -> Void

    var body: some View {
        Button(action: tick) {
            HStack(spacing: 12) {
                ArtworkView(track: higher.song, corner: 6)
                    .frame(width: artworkEdge, height: artworkEdge)
                VStack(alignment: .leading, spacing: 2) {
                    Text(higher.song.shownTitle).titleLines()
                    Text(HigherVersionRows.line(higher)).font(.subheadline).foregroundStyle(.secondary).titleLines(2)
                }
                Spacer(minLength: 0)
                Image(systemName: ticked ? "checkmark.circle.fill" : "circle")
                    .foregroundStyle(ticked ? Color.worn : Color.secondary)
                    .accessibilityHidden(true)
            }
            .contentShape(.rect)
        }
        .buttonStyle(.plain)
        .accessibilityValue(said(ticked ? "higher.ticked" : "higher.unticked"))
        .accessibilityHint(said("higher.tick.hint"))
    }
}
