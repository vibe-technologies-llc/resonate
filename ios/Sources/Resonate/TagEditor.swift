import ResonateLogic
import ResonateNative
import SwiftUI

struct TagFailure: LocalizedError {
    let text: String

    var errorDescription: String? { text }
}

struct TagEditing: Identifiable {
    let id = UUID()
    let tracks: [Track]
}

struct TagEditorSheet: View {
    private static let readAtMost = 500

    @Environment(Library.self) private var library
    @Environment(\.dismiss) private var dismiss
    let tracks: [Track]
    @State private var shown: [TagName: String] = [:]
    @State private var read: [TagName: String] = [:]
    @State private var mixed: Set<TagName> = []
    @State private var loaded = false
    @State private var failure: String?
    @State private var discarding = false
    @State private var writableTracks: [Track]?

    var body: some View {
        let features = library.features
        let writable = writableTracks ?? []
        let names = TagName.allCases.filter { tracks.count == 1 || !$0.eachSongsOwn }
        NavigationStack {
            Form {
                if writableTracks == nil {
                    Section { ProgressView().frame(maxWidth: .infinity) }
                } else if writable.isEmpty {
                    Section {
                        Text("Tags of this kind of file cannot be written.").foregroundStyle(.secondary)
                    }
                } else if let failure {
                    Section { ReadFailure(text: failure) { Task { await load(writable) } } }
                } else if !loaded {
                    Section { ProgressView().frame(maxWidth: .infinity) }
                } else {
                    Section {
                        ForEach(names.filter { !$0.numbered }, id: \.self) { field($0) }
                    } footer: {
                        if tracks.count > 1 {
                            Text("Fields left as they are stay as each song has them; a field marked Mixed changes only once you type into it.")
                        }
                    }
                    Section {
                        HStack {
                            if tracks.count == 1 { field(.trackNumber) }
                            field(.trackTotal)
                        }
                        HStack {
                            field(.discNumber)
                            field(.discTotal)
                        }
                    } header: {
                        Text("Numbers")
                    } footer: {
                        if writable.count < tracks.count {
                            Text("\(counted(tracks.count - writable.count, "song")) cannot be written and will be left alone.")
                        }
                    }
                }
            }
            .navigationTitle(tracks.count == 1 ? "Edit tags" : "Edit \(counted(tracks.count, "song"))")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button(role: .close) {
                        if changed.isEmpty { dismiss() } else { discarding = true }
                    }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Save", role: .confirm) {
                        features.editTags(writable, change)
                        dismiss()
                    }
                    .disabled(!loaded || writable.isEmpty || changed.isEmpty)
                }
            }
        }
        .keepingEditsUnlessDiscarded(!changed.isEmpty, asking: $discarding) { dismiss() }
        .task {
            let asked = tracks.filter(features.tagsWritable)
            writableTracks = asked
            await load(asked)
        }
    }

    private func field(_ name: TagName) -> some View {
        let binding = Binding(get: { shown[name] ?? "" }, set: { shown[name] = $0 })
        return LabeledContent(name.label) {
            TextField(mixed.contains(name) ? "Mixed" : name.label, text: binding)
                .multilineTextAlignment(.trailing)
                .keyboardType(name.numbered ? .numberPad : .default)
                .textInputAutocapitalization(name.numbered ? .never : .words)
                .autocorrectionDisabled()
        }
    }

    private var changed: [TagName] {
        TagDiff.changed(shown: shown, read: read, mixed: mixed)
    }

    private var change: SongTags {
        TagDiff.change(shown: shown, changed: changed)
    }

    private func load(_ writable: [Track]) async {
        guard !writable.isEmpty else { return }
        let all: [SongTags]
        do {
            all = try await library.features.songTags(writable.prefix(Self.readAtMost).map(\.id))
        } catch {
            failure = explained(error)
            return
        }
        failure = nil
        let (common, differing) = TagDiff.common(of: all)
        read = common
        shown = common
        mixed = differing
        loaded = true
    }
}
