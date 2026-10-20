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

enum TagName: CaseIterable, Hashable {
    case title, artist, album, albumArtist, genre, date, trackNumber, trackTotal, discNumber, discTotal, composer

    var label: String {
        switch self {
        case .title: "Title"
        case .artist: "Artist"
        case .album: "Album"
        case .albumArtist: "Album artist"
        case .genre: "Genre"
        case .date: "Year or date"
        case .trackNumber: "Track"
        case .trackTotal: "of"
        case .discNumber: "Disc"
        case .discTotal: "of"
        case .composer: "Composer"
        }
    }

    var numbered: Bool {
        switch self {
        case .trackNumber, .trackTotal, .discNumber, .discTotal: true
        default: false
        }
    }

    var eachSongsOwn: Bool {
        self == .title || self == .trackNumber
    }

    var path: WritableKeyPath<SongTags, String?> {
        switch self {
        case .title: \.title
        case .artist: \.artist
        case .album: \.album
        case .albumArtist: \.albumArtist
        case .genre: \.genre
        case .date: \.date
        case .trackNumber: \.trackNumber
        case .trackTotal: \.trackTotal
        case .discNumber: \.discNumber
        case .discTotal: \.discTotal
        case .composer: \.composer
        }
    }
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
                    Button(role: .close) { dismiss() }
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
        TagName.allCases.filter { name in
            let now = shown[name] ?? ""
            return mixed.contains(name) ? !now.isEmpty : now != (read[name] ?? "")
        }
    }

    private var change: SongTags {
        changed.reduce(into: SongTags()) { tags, name in
            tags[keyPath: name.path] = shown[name] ?? ""
        }
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
        var common: [TagName: String] = [:]
        var differing: Set<TagName> = []
        for name in TagName.allCases {
            let values = Set(all.map { $0[keyPath: name.path] ?? "" })
            if values.count == 1, let only = values.first {
                common[name] = only
            } else if values.count > 1 {
                differing.insert(name)
            }
        }
        read = common
        shown = common
        mixed = differing
        loaded = true
    }
}
