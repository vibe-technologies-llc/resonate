import ResonateLogic
import ResonateNative
import SwiftUI
import UniformTypeIdentifiers

@MainActor
final class PlaylistEntries {
    private var rows: [Track] = []
    private var entries: [PlaylistEntry] = []

    func of(_ shown: [Track]) -> [PlaylistEntry] {
        if shown != rows {
            rows = shown
            entries = PlaylistEntry.of(shown)
        }
        return entries
    }
}

struct PlaylistFile: FileDocument {
    static let readableContentTypes: [UTType] = [.plainText]

    let text: String
    let kind: UTType

    init(text: String, kind: UTType) {
        self.text = text
        self.kind = kind
    }

    init(configuration: ReadConfiguration) throws {
        text = String(decoding: configuration.file.regularFileContents ?? Data(), as: UTF8.self)
        kind = .plainText
    }

    func fileWrapper(configuration: WriteConfiguration) throws -> FileWrapper {
        FileWrapper(regularFileWithContents: Data(text.utf8))
    }
}

extension PlaylistFileFormat {
    static let offered: [PlaylistFileFormat] = [.m3u, .pls, .xspf]

    var title: String {
        switch self {
        case .m3u: "M3U8"
        case .pls: "PLS"
        case .xspf: "XSPF"
        }
    }

    var fileExtension: String {
        switch self {
        case .m3u: "m3u8"
        case .pls: "pls"
        case .xspf: "xspf"
        }
    }

    var kind: UTType {
        UTType(filenameExtension: fileExtension) ?? .plainText
    }
}

func playlistFileTypes() -> [UTType] {
    playlistFileExtensions().compactMap { UTType(filenameExtension: $0) }
}

struct PlaylistView: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    @Environment(\.dismiss) private var dismiss
    let id: UInt64
    @State private var renaming = false
    @State private var deleting = false
    @State private var adding = false
    @State private var seen = false
    @State private var edited = 0
    @State private var choosingPlaylist = false
    @State private var queued = 0
    @State private var exported: PlaylistFile?
    @State private var exporting = false
    @State private var entries = PlaylistEntries()

    var body: some View {
        let features = library.features
        let playlist = features.playlists.first { $0.id == id }
        let rows = features.openPlaylist == id ? features.playlistRows : []
        let name = playlist?.name ?? "Playlist"
        List {
            CollectionHeader(title: name, subtitle: "Playlist", tracks: rows) {
                PlaylistArtwork(playlist: playlist, corner: 20)
            }
            if rows.isEmpty, playlist?.songs == 0 {
                VStack(spacing: 12) {
                    Text("Your playlist starts here. Add songs from your library to make it yours.")
                        .multilineTextAlignment(.center)
                        .foregroundStyle(.secondary)
                    Button("Add songs", systemImage: "plus") { adding = true }
                        .buttonStyle(.glass)
                }
                .frame(maxWidth: .infinity)
                .listRowBackground(Color.clear)
                .listRowSeparator(.hidden)
            }
            ForEach(entries.of(rows)) { entry in
                PlaylistRow(entry: entry, count: rows.count, move: move, remove: remove)
                    .equatable()
                    .listRowBackground(Color.clear)
            }
            .onMove { from, to in
                guard let start = from.first else { return }
                move(start, to: to > start ? to - 1 : to)
            }
            .onDelete { offsets in
                for position in offsets.sorted(by: >) { remove(position) }
            }
        }
        .listStyle(.plain)
        .background(BackdropView(track: rows.first, moving: false))
        .scrollContentBackground(.hidden)
        .navigationTitle(name)
        .navigationBarTitleDisplayMode(.inline)
        .sensoryFeedback(.selection, trigger: edited)
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                Button("Add songs", systemImage: "plus") { adding = true }
            }
            ToolbarSpacer(.fixed, placement: .topBarTrailing)
            ToolbarItem(placement: .topBarTrailing) { EditButton() }
            ToolbarItem(placement: .topBarTrailing) {
                Menu("Playlist options", systemImage: "ellipsis") {
                    if !rows.isEmpty {
                        CollectionQueueActions(tracks: rows, choosingPlaylist: $choosingPlaylist, queued: $queued)
                    }
                    PlaylistOptions(id: id, renaming: $renaming, deleting: $deleting, export: export)
                }
                .simultaneousGesture(TapGesture().onEnded { features.loadPlaylistSync(id) })
            }
        }
        .onAppear {
            features.selectPlaylist(id)
            features.loadPlaylistSync(id)
        }
        .onDisappear {
            if !navigation.paths.values.contains(where: { $0.contains(.playlist(id)) }) {
                features.leavePlaylist(id)
            }
        }
        .onChange(of: playlist == nil, initial: true) { _, missing in
            if !missing {
                seen = true
            } else if seen {
                dismiss()
            }
        }
        .alert("Rename playlist", isPresented: $renaming) {
            PlaylistNameField(initial: playlist?.name ?? "") { name in
                edit { native, revision in try native.renamePlaylist(id: id, revision: revision, name: name) }
            }
        }
        .confirmationDialog("Delete \(name)?", isPresented: $deleting, titleVisibility: .visible) {
            Button("Delete playlist", role: .destructive) {
                edit { native, revision in try native.deletePlaylist(id: id, revision: revision) }
            }
        } message: {
            Text("The music files stay in your library. You can undo this action from a playlist's options.")
        }
        .sheet(isPresented: $adding) { AddSongsSheet(id: id) }
        .fileExporter(
            isPresented: $exporting,
            document: exported,
            contentType: exported?.kind ?? .plainText,
            defaultFilename: name
        ) { result in
            if case .failure(let failure) = result { library.report(failure) }
            exported = nil
        }
        .sheet(isPresented: $choosingPlaylist) { PlaylistPicker(tracks: rows) }
        .sensoryFeedback(.success, trigger: queued)
    }

    private func export(_ format: PlaylistFileFormat) {
        let id = id
        Task {
            do {
                let native = try await library.player()
                let text = try await Work.aside { try native.exportPlaylist(id: id, format: format) }
                exported = PlaylistFile(text: text, kind: format.kind)
                exporting = true
            } catch {
                library.report(error)
            }
        }
    }

    private func edit(_ change: @escaping @Sendable (MobilePlayer, UInt64) throws -> Void) {
        let id = id
        library.features.editPlaylist { native in
            guard let revision = try native.playlists().first(where: { $0.id == id })?.revision else { return }
            try change(native, revision)
        }
    }

    private func move(_ from: Int, to: Int) {
        let rows = library.features.playlistRows
        guard rows.indices.contains(from), rows.indices.contains(to), from != to else { return }
        var moved = rows
        moved.insert(moved.remove(at: from), at: to)

        edited += 1
        library.features.playlistRows = moved
        edit { native, revision in
            try native.moveInPlaylist(id: id, revision: revision, from: UInt32(from), to: UInt32(to))
        }
    }

    private func remove(_ position: Int) {
        guard library.features.playlistRows.indices.contains(position) else { return }

        edited += 1
        library.features.playlistRows.remove(at: position)
        edit { native, revision in
            try native.removeFromPlaylist(id: id, revision: revision, position: UInt32(position))
        }
    }
}

struct PlaylistOptions: View {
    @Environment(Library.self) private var library
    let id: UInt64
    @Binding var renaming: Bool
    @Binding var deleting: Bool
    let export: (PlaylistFileFormat) -> Void

    var body: some View {
        let features = library.features
        let sync = features.playlistSync.flatMap { $0.playlist == id ? $0 : nil }
        let devices = (features.syncStatus?.devices ?? []).filter { device in
            !device.thisDevice && (device.nearby || device.online && features.online)
        }
        Section {
            Button("Rename playlist", systemImage: "pencil") { renaming = true }
            Button("Undo last playlist edit", systemImage: "arrow.uturn.backward") { features.undoPlaylistEdit() }
            Menu("Export playlist", systemImage: "square.and.arrow.up") {
                ForEach(PlaylistFileFormat.offered, id: \.self) { format in
                    Button(format.title) { export(format) }
                }
            }
            Button("Delete playlist", systemImage: "trash", role: .destructive) { deleting = true }
        }
        if let sync, features.syncStatus?.standing != .off {
            Section("Your devices") {
                Toggle("Keep its songs here", isOn: Binding(get: { sync.kept }, set: { features.keepSongs(id, $0) }))
                if sync.missing > 0 {
                    ForEach(devices, id: \.name) { device in
                        Button("Fetch \(counted(Int(sync.missing), "missing song")) from \(device.name)") {
                            features.fetchMissing(id, from: device)
                        }
                    }
                }
            }
        }
    }
}

struct PlaylistRow: View, Equatable {
    @Environment(Library.self) private var library
    let entry: PlaylistEntry
    let count: Int
    let move: (Int, Int) -> Void
    let remove: (Int) -> Void
    @State private var choosingPlaylist = false
    @State private var queued = 0

    var body: some View {
        let track = entry.track
        let position = entry.position
        let favourite = library.isFavourite(track)
        TrackRow(track: track, ownsMenus: false) {
            library.play(library.features.playlistRows, at: position)
        }
        .swipeActions(edge: .trailing) {
            Button("Remove from playlist", systemImage: "minus.circle", role: .destructive) { remove(position) }
                .tint(.red)
        }
        .contextMenu {
            Section {
                Button("Play next", systemImage: "text.line.first.and.arrowtriangle.forward") {
                    queued += 1
                    library.playNext([track])
                }
                Button("Add to queue", systemImage: "text.line.last.and.arrowtriangle.forward") {
                    queued += 1
                    library.addToQueue([track])
                }
            }
            Section {
                Button("Add to another playlist", systemImage: "text.badge.plus") { choosingPlaylist = true }
                Button(favourite ? "Remove from favourites" : "Add to favourites", systemImage: favourite ? "heart.slash" : "heart") {
                    library.setFavourite(track, !favourite)
                }
            }
            Section {
                SongPages(track: track)
            }
            Section {
                if position > 0 {
                    Button("Move up", systemImage: "arrow.up") { move(position, position - 1) }
                }
                if position + 1 < count {
                    Button("Move down", systemImage: "arrow.down") { move(position, position + 1) }
                }
                Button("Remove from playlist", systemImage: "minus.circle", role: .destructive) { remove(position) }
            }
        }
        .sensoryFeedback(.success, trigger: queued)
        .sheet(isPresented: $choosingPlaylist) { PlaylistPicker(tracks: [track]) }
    }

    nonisolated static func == (shown: PlaylistRow, other: PlaylistRow) -> Bool {
        shown.entry == other.entry && shown.count == other.count
    }
}

@MainActor
@Observable
final class ChosenSongs {
    private(set) var ids: [UInt64] = []
    private(set) var held: Set<UInt64> = []

    func toggle(_ id: UInt64) {
        if let at = ids.firstIndex(of: id) {
            ids.remove(at: at)
            held.remove(id)
        } else {
            ids.append(id)
            held.insert(id)
        }
    }
}

struct AddSongsSheet: View {
    private static let settles: Duration = .milliseconds(150)

    @Environment(Library.self) private var library
    @Environment(\.dismiss) private var dismiss
    let id: UInt64
    @State private var search = ""
    @State private var chosen = ChosenSongs()
    @State private var keyed = SongKeys.empty
    @State private var listed: [Track] = []
    @State private var keying = true

    var body: some View {
        NavigationStack {
            List(listed, id: \.id) { track in
                ChoosableSong(track: track, chosen: chosen)
            }
            .overlay {
                if keying || !library.catalogRead {
                    ProgressView()
                } else if keyed.tracks.isEmpty {
                    ContentUnavailableView("Add a music folder in Settings to start your library.", systemImage: "music.note")
                } else if listed.isEmpty {
                    NoneMatch(kind: "songs", query: search, symbol: "magnifyingglass")
                }
            }
            .searchable(text: $search, prompt: "Find songs to add")
            .navigationTitle("Add songs")
            .navigationBarTitleDisplayMode(.inline)
            .task(id: library.catalogEpoch) {
                let tracks = library.allTracks
                keyed = (try? await Work.aside { SongKeys.of(tracks) }) ?? keyed
                keying = false
                listed = keyed.matching(Self.folded(search))
            }
            .task(id: search) {
                if !search.isEmpty { try? await Task.sleep(for: Self.settles) }
                guard !Task.isCancelled else { return }
                let keys = keyed
                let folded = Self.folded(search)
                let found = (try? await Work.aside { keys.matching(folded) }) ?? []
                guard !Task.isCancelled else { return }
                listed = found
            }
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button(role: .close) { dismiss() } }
                ToolbarItem(placement: .confirmationAction) {
                    AddChosenButton(chosen: chosen) {
                        let byId = Dictionary(keyed.tracks.map { ($0.id, $0) }, uniquingKeysWith: { kept, _ in kept })
                        let picked = chosen.ids.compactMap { byId[$0] }
                        library.features.addToPlaylist(id, picked)
                        dismiss()
                    }
                }
            }
        }
    }

    private static func folded(_ search: String) -> String {
        search.trimmingCharacters(in: .whitespacesAndNewlines).searchFolded
    }
}

struct ChoosableSong: View {
    let track: Track
    let chosen: ChosenSongs

    var body: some View {
        let isChosen = chosen.held.contains(track.id)
        Button {
            chosen.toggle(track.id)
        } label: {
            HStack {
                CollectionRow(track: track, title: track.shownTitle, detail: track.shownArtist)
                Spacer()
                Image(systemName: isChosen ? "checkmark.circle.fill" : "circle")
                    .foregroundStyle(.tint)
            }
        }
        .buttonStyle(.plain)
        .accessibilityAddTraits(isChosen ? .isSelected : [])
    }
}

struct AddChosenButton: View {
    let chosen: ChosenSongs
    let add: () -> Void

    var body: some View {
        let count = chosen.ids.count
        Button(count == 1 ? "Add 1 song" : "Add \(count) songs", role: .confirm, action: add)
            .disabled(count == 0)
    }
}

struct PlaylistPicker: View {
    @Environment(Library.self) private var library
    @Environment(\.dismiss) private var dismiss
    @ScaledMetric(relativeTo: .body) private var artworkEdge: CGFloat = 44
    let tracks: [Track]
    @State private var name = ""
    @State private var asking = false
    @State private var failure: String?

    var body: some View {
        let trimmed = name.trimmingCharacters(in: .whitespacesAndNewlines)
        NavigationStack {
            List {
                Section {
                    TextField("New playlist name", text: $name)
                        .submitLabel(.done)
                        .onSubmit { create(trimmed) }
                    Button("Create and add", systemImage: "plus") { create(trimmed) }
                        .disabled(trimmed.isEmpty || asking)
                } footer: {
                    if let failure { Text(failure).foregroundStyle(.red) }
                }
                Section {
                    ForEach(library.features.playlists, id: \.id) { playlist in
                        Button {
                            asking = true
                            library.features.addToPlaylist(playlist.id, tracks) { finished($0, playlist.name) }
                        } label: {
                            HStack(spacing: 12) {
                                PlaylistArtwork(playlist: playlist).frame(width: artworkEdge, height: artworkEdge)
                                VStack(alignment: .leading, spacing: 2) {
                                    Text(playlist.name).lineLimit(1)
                                    Text(counted(Int(playlist.songs), "song")).font(.subheadline).foregroundStyle(.secondary)
                                }
                            }
                            .accessibilityElement(children: .combine)
                        }
                        .buttonStyle(.plain)
                        .disabled(asking)
                    }
                }
            }
            .navigationTitle("Add to playlist")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button(role: .close) { dismiss() } }
            }
            .onAppear { library.features.loadPlaylists() }
        }
        .presentationDetents([.medium, .large])
    }

    private func create(_ trimmed: String) {
        guard !trimmed.isEmpty, !asking else { return }
        asking = true
        library.features.createPlaylist(trimmed, with: tracks) { finished($0, trimmed) }
    }

    private func finished(_ problem: String?, _ playlist: String) {
        asking = false
        failure = problem
        guard problem == nil else { return }
        library.notice("Added to \(playlist)")
        dismiss()
    }
}
