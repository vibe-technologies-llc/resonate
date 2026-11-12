import ResonateNative
import SwiftUI

struct VersionsOf: Identifiable {
    let id = UUID()
    let track: Track
}

struct VersionsOffered: Sendable {
    let searches: [VersionSearchOffered]
    let chosen: Bool
}

enum VersionsShown {
    case reading
    case offered(VersionsOffered)
    case looking(VersionSearchOffered)
    case found(FoundVersions)
    case failed(String)
}

struct VersionsSheet: View {
    @Environment(Library.self) private var library
    @Environment(\.dismiss) private var dismiss
    let track: Track
    @State private var shown = VersionsShown.reading
    @State private var asked = 0

    var body: some View {
        NavigationStack {
            List {
                Section {
                    content
                } header: {
                    Text(track.shownTitle).textCase(nil)
                }
            }
            .navigationTitle("versions.title")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button(role: .close) { dismiss() }
                }
            }
        }
        .task(id: asked) { await read() }
    }

    @ViewBuilder
    private var content: some View {
        switch shown {
        case .reading:
            ProgressView().accessibilityLabel("versions.reading").frame(maxWidth: .infinity)
        case .offered(let offered):
            ForEach(offered.searches, id: \.label) { search in
                Button(search.label, systemImage: "magnifyingglass") { look(search) }
            }
            if offered.chosen {
                Button("versions.play-own-copy", systemImage: "arrow.uturn.backward") {
                    library.features.forgetVersionChosen(track)
                    dismiss()
                }
            }
        case .looking(let search):
            HStack(spacing: 12) {
                ProgressView().accessibilityLabel("versions.looking")
                Text(search.label).foregroundStyle(.secondary)
            }
        case .found(let found):
            if let trouble = found.trouble {
                Label(trouble, systemImage: "exclamationmark.triangle").foregroundStyle(.secondary)
            }
            if let said = found.said {
                Text(said).foregroundStyle(.secondary)
            }
            ForEach(Array(found.versions.enumerated()), id: \.offset) { _, version in
                Button(version.label, systemImage: "play.fill") {
                    library.features.chooseVersion(track, version.version)
                    dismiss()
                }
            }
        case .failed(let failure):
            ReadFailure(text: failure) { asked += 1 }
        }
    }

    private func read() async {
        shown = .reading
        do {
            shown = .offered(try await library.features.versionSearches(track))
        } catch {
            shown = .failed(explained(error))
        }
    }

    private func look(_ search: VersionSearchOffered) {
        shown = .looking(search)
        Task {
            do {
                shown = .found(try await library.features.findVersions(track, search.search))
            } catch {
                shown = .failed(explained(error))
            }
        }
    }
}
