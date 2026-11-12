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

enum PlaylistFileTypes {
    static let offered = playlistFileExtensions().compactMap { UTType(filenameExtension: $0) }
}

struct PlaylistView: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    @Environment(\.dismiss) private var dismiss
    let id: UInt64
    @State private var renaming = false
    @State private var revising = false
    @State private var deleting = false
    @State private var adding = false
    @State private var seen = false
    @State private var edited = 0
    @State private var choosingPlaylist = false
    @State private var queued = 0
    @State private var exported: PlaylistFile?
    @State private var exporting = false
    @State private var entries = PlaylistEntries()
    @State private var sharingList = false

    var body: some View {
        let features = library.features
        let playlist = features.playlists.first { $0.id == id }
        let rows = features.openPlaylist == id ? features.playlistRows : []
        let name = playlist?.name ?? said("playlist.kind")
        let search = playlist?.search
        let shared = features.together.shared(id)
        let followed = playlist?.followed ?? false
        let mayEdit = SharedPlaylistWords.mayEdit(shared: shared?.mayEdit, followed: followed)
        let subtitle = search == nil
            ? SharedPlaylistWords.kind(shared: shared?.mayEdit, followed: followed) ?? playlist?.link.map(LinkedPlaylistWords.header) ?? said("playlist.kind")
            : said("playlist.saved-search")
        List {
            CollectionHeader(title: name, subtitle: subtitle, tracks: rows) {
                PlaylistArtwork(playlist: playlist, corner: 20)
            }
            if let search {
                SavedSearchRows(search: search, rows: rows, entries: entries.of(rows))
            } else {
                listRows(rows, empty: playlist?.songs == 0, mayEdit: mayEdit)
            }
        }
        .listStyle(.plain)
        .background(BackdropView(track: rows.first, moving: false))
        .scrollContentBackground(.hidden)
        .navigationTitle(name)
        .navigationBarTitleDisplayMode(.inline)
        .sensoryFeedback(.selection, trigger: edited)
        .toolbar {
            if search == nil, mayEdit {
                ToolbarItem(placement: .topBarTrailing) {
                    Button("playlist.add-songs", systemImage: "plus") { adding = true }
                }
                ToolbarSpacer(.fixed, placement: .topBarTrailing)
                ToolbarItem(placement: .topBarTrailing) { EditButton() }
            }
            ToolbarItem(placement: .topBarTrailing) {
                Menu("playlist.options", systemImage: "ellipsis") {
                    if !rows.isEmpty {
                        CollectionQueueActions(tracks: rows, choosingPlaylist: $choosingPlaylist, queued: $queued)
                    }
                    PlaylistOptions(
                        id: id,
                        fills: search != nil,
                        link: playlist?.link,
                        mayEdit: mayEdit,
                        renaming: $renaming,
                        revising: $revising,
                        deleting: $deleting,
                        sharing: $sharingList,
                        export: export
                    )
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
        .alert("playlist.rename", isPresented: $renaming) {
            PlaylistNameField(initial: playlist?.name ?? "") { name in
                edit { native, revision in try native.renamePlaylist(id: id, revision: revision, name: name) }
            }
        }
        .sheet(isPresented: $revising) {
            if let search {
                SavedSearchSheet(title: said("playlist.saved-search.edit"), draft: .of(name: name, search: search)) { renamed, revised in
                    library.features.reviseSearch(id, name: renamed, search: revised) { problem in
                        if let problem { library.fail(problem) }
                    }
                }
            }
        }
        .confirmationDialog(said("playlist.delete.title", name), isPresented: $deleting, titleVisibility: .visible) {
            Button("playlist.delete", role: .destructive) {
                edit { native, revision in try native.deletePlaylist(id: id, revision: revision) }
            }
        } message: {
            Text("playlist.delete.message")
        }
        .sheet(isPresented: $adding) { AddSongsSheet(id: id).sheetSized(.long) }
        .sheet(isPresented: $sharingList) { SharedPlaylistSheet(id: id, name: name).sheetSized(.long) }
        .fileExporter(
            isPresented: $exporting,
            document: exported,
            contentType: exported?.kind ?? .plainText,
            defaultFilename: name
        ) { result in
            if case .failure(let failure) = result { library.report(failure) }
            exported = nil
        }
        .sheet(isPresented: $choosingPlaylist) { PlaylistPicker(tracks: rows).sheetSized() }
        .sensoryFeedback(.success, trigger: queued)
    }

    @ViewBuilder
    private func listRows(_ rows: [Track], empty: Bool, mayEdit: Bool) -> some View {
        if rows.isEmpty, empty, mayEdit {
            VStack(spacing: 12) {
                Text("playlist.empty")
                    .multilineTextAlignment(.center)
                    .foregroundStyle(.secondary)
                Button("playlist.add-songs", systemImage: "plus") { adding = true }
                    .buttonStyle(.glass)
            }
            .frame(maxWidth: .infinity)
            .listRowBackground(Color.clear)
            .listRowSeparator(.hidden)
        }
        ForEach(entries.of(rows)) { entry in
            PlaylistRow(entry: entry, count: rows.count, mayEdit: mayEdit, move: move, remove: remove)
                .equatable()
                .listRowBackground(Color.clear)
        }
        .onMove(perform: mayEdit ? { from, to in
            guard let start = from.first else { return }
            move(start, to: RowMoves.playlist(from: start, destination: to))
        } : nil)
        .onDelete(perform: mayEdit ? { offsets in
            for position in offsets.sorted(by: >) { remove(position) }
        } : nil)
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
    var fills = false
    var link: PlaylistLinkInfo?
    var mayEdit = true
    @Binding var renaming: Bool
    @Binding var revising: Bool
    @Binding var deleting: Bool
    @Binding var sharing: Bool
    let export: (PlaylistFileFormat) -> Void

    var body: some View {
        let features = library.features
        let sync = features.playlistSync.flatMap { $0.playlist == id ? $0 : nil }
        let devices = (features.syncSummary?.devices ?? []).filter { device in
            !device.thisDevice && (device.nearby || device.online && features.online)
        }
        Section {
            if fills {
                Button("playlist.edit-search", systemImage: "slider.horizontal.3") { revising = true }
            }
            if mayEdit {
                Button("playlist.rename", systemImage: "pencil") { renaming = true }
                Button("playlists.undo", systemImage: "arrow.uturn.backward") { features.undoPlaylistEdit() }
            }
            if !fills {
                Button("playlist.share", systemImage: "person.2") { sharing = true }
            }
            Menu("playlist.export", systemImage: "square.and.arrow.up") {
                ForEach(PlaylistFileFormat.offered, id: \.self) { format in
                    Button(format.title) { export(format) }
                }
            }
            if mayEdit {
                Button("playlist.delete", systemImage: "trash", role: .destructive) { deleting = true }
            }
        }
        if let link, !fills {
            Section(LinkedPlaylistWords.header(link)) {
                Button(LinkedPlaylistWords.refreshFrom(link.service), systemImage: "arrow.clockwise") {
                    features.refreshLinkedPlaylist(id)
                }
                .disabled(!features.online)
                Toggle(
                    "playlist.link.keep-in-step",
                    systemImage: "arrow.triangle.2.circlepath",
                    isOn: Binding(get: { link.keptInStep }, set: { features.keepPlaylistInStep(id, $0) })
                )
                Button("playlist.link.forget", systemImage: "link") { features.forgetPlaylistLink(id) }
            }
        }
        if let sync, !fills, features.syncSummary?.standing != .off {
            Section("playlist.devices") {
                Toggle("playlist.devices.keep-songs", isOn: Binding(get: { sync.kept }, set: { features.keepSongs(id, $0) }))
                if sync.missing > 0 {
                    ForEach(devices, id: \.id) { device in
                        Button(said("playlist.devices.fetch-missing", Int(sync.missing), device.name)) {
                            features.fetchMissing(id, from: device)
                        }
                    }
                }
            }
        }
    }
}

struct SavedSearchRows: View {
    @Environment(Library.self) private var library
    let search: SavedSearch
    let rows: [Track]
    let entries: [PlaylistEntry]

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(search.text.isEmpty ? said("playlist.saved-search.every-song") : search.text)
                .font(.subheadline)
            Text(search.reading)
                .font(.footnote)
        }
        .foregroundStyle(.secondary)
        .accessibilityElement(children: .combine)
        .listRowBackground(Color.clear)
        .listRowSeparator(.hidden)
        if rows.isEmpty {
            Text("playlist.saved-search.empty")
                .multilineTextAlignment(.center)
                .foregroundStyle(.secondary)
                .frame(maxWidth: .infinity)
                .listRowBackground(Color.clear)
                .listRowSeparator(.hidden)
        }
        ForEach(entries) { entry in
            TrackRow(track: entry.track) {
                library.play(rows, at: entry.position)
            }
            .listRowBackground(Color.clear)
        }
    }
}

struct PlaylistRow: View, Equatable {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    let entry: PlaylistEntry
    let count: Int
    let mayEdit: Bool
    let move: (Int, Int) -> Void
    let remove: (Int) -> Void

    var body: some View {
        let track = entry.track
        let position = entry.position
        let favourite = library.isFavourite(track)
        TrackRow(track: track, ownsMenus: false) {
            library.play(library.features.playlistRows, at: position)
        }
        .swipeActions(edge: .trailing) {
            if mayEdit {
                Button("playlist.remove-song", systemImage: "minus.circle", role: .destructive) { remove(position) }
                    .tint(.red)
            }
        }
        .contextMenu {
            Section {
                Button("song.play-next", systemImage: "text.line.first.and.arrowtriangle.forward") {
                    navigation.feelQueued()
                    library.playNext([track])
                }
                Button("song.add-to-queue", systemImage: "text.line.last.and.arrowtriangle.forward") {
                    navigation.feelQueued()
                    library.addToQueue([track])
                }
            }
            Section {
                Button("playlist.add-to-another", systemImage: "text.badge.plus") { navigation.addToPlaylist([track]) }
                Button(said(favourite ? "song.favourite.remove" : "song.favourite.add"), systemImage: favourite ? "heart.slash" : "heart") {
                    library.setFavourite(track, !favourite)
                }
            }
            Section {
                SongPages(track: track)
            }
            if mayEdit {
                Section {
                    if position > 0 {
                        Button("playlist.move-up", systemImage: "arrow.up") { move(position, position - 1) }
                    }
                    if position + 1 < count {
                        Button("playlist.move-down", systemImage: "arrow.down") { move(position, position + 1) }
                    }
                    Button("playlist.remove-song", systemImage: "minus.circle", role: .destructive) { remove(position) }
                }
            }
        }
    }

    nonisolated static func == (shown: PlaylistRow, other: PlaylistRow) -> Bool {
        shown.entry == other.entry && shown.count == other.count && shown.mayEdit == other.mayEdit
    }
}

@MainActor
@Observable
final class ChosenSongs {
    private(set) var ids: [UInt64] = []
    @ObservationIgnored private var flags: [UInt64: ChosenFlag] = [:]

    func toggle(_ id: UInt64) {
        if let at = ids.firstIndex(of: id) {
            ids.remove(at: at)
            flag(of: id).on = false
        } else {
            ids.append(id)
            flag(of: id).on = true
        }
    }

    func isChosen(_ id: UInt64) -> Bool {
        flag(of: id).on
    }

    private func flag(of id: UInt64) -> ChosenFlag {
        if let kept = flags[id] { return kept }
        let made = ChosenFlag()
        flags[id] = made
        return made
    }
}

@MainActor
@Observable
final class ChosenFlag {
    var on = false
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
    @State private var discarding = false

    var body: some View {
        NavigationStack {
            List(listed, id: \.id) { track in
                ChoosableSong(track: track, chosen: chosen)
            }
            .overlay {
                if keying || !library.catalogRead {
                    ProgressView().accessibilityLabel("playlist.adding.reading")
                } else if keyed.tracks.isEmpty {
                    ContentUnavailableView("playlist.adding.no-library", systemImage: "music.note")
                } else if listed.isEmpty {
                    NoneMatch(kind: "songs", query: search, symbol: "magnifyingglass")
                }
            }
            .searchable(text: $search, prompt: "playlist.adding.search.prompt")
            .navigationTitle("playlist.add-songs")
            .navigationBarTitleDisplayMode(.inline)
            .task(id: library.catalogEpoch) {
                let tracks = library.allTracks
                let folded = Self.folded(search)
                guard let (keys, found) = try? await Work.onScreen({ () -> (SongKeys, [Track]) in
                    let keys = SongKeys.of(tracks)
                    return (keys, keys.matching(folded))
                }) else { return }
                guard !Task.isCancelled else { return }
                keyed = keys
                keying = false
                listed = found
            }
            .task(id: search) {
                if !search.isEmpty { try? await Task.sleep(for: Self.settles) }
                guard !Task.isCancelled else { return }
                let keys = keyed
                let folded = Self.folded(search)
                let found = (try? await Work.onScreen { keys.matching(folded) }) ?? []
                guard !Task.isCancelled else { return }
                listed = found
            }
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button(role: .close) {
                        if chosen.ids.isEmpty { dismiss() } else { discarding = true }
                    }
                }
                ToolbarItem(placement: .confirmationAction) {
                    AddChosenButton(chosen: chosen) {
                        let picked = chosen.ids.compactMap { library.track($0) }
                        library.features.addToPlaylist(id, picked)
                        dismiss()
                    }
                }
            }
        }
        .keepingEditsUnlessDiscarded(!chosen.ids.isEmpty, asking: $discarding) { dismiss() }
    }

    private static func folded(_ search: String) -> String {
        search.trimmingCharacters(in: .whitespacesAndNewlines).searchFolded
    }
}

struct ChoosableSong: View {
    let track: Track
    let chosen: ChosenSongs

    var body: some View {
        let isChosen = chosen.isChosen(track.id)
        Button {
            chosen.toggle(track.id)
        } label: {
            HStack {
                CollectionRow(track: track, title: track.shownTitle, detail: track.shownArtist)
                Spacer()
                Image(systemName: isChosen ? "checkmark.circle.fill" : "circle")
                    .foregroundStyle(.tint)
                    .accessibilityHidden(true)
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
        Button(said("playlist.adding.add", count), role: .confirm, action: add)
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
                    TextField("playlist.picker.new-name", text: $name)
                        .submitLabel(.done)
                        .onSubmit { create(trimmed) }
                    Button("playlist.picker.create-and-add", systemImage: "plus") { create(trimmed) }
                        .disabled(trimmed.isEmpty || asking)
                } footer: {
                    if let failure { Text(failure).foregroundStyle(Color.failure).announced(failure) }
                }
                Section {
                    ForEach(library.features.playlists.filter { $0.search == nil && library.features.together.mayEdit($0.id) }, id: \.id) { playlist in
                        Button {
                            asking = true
                            library.features.addToPlaylist(playlist.id, tracks) { finished($0, playlist.name) }
                        } label: {
                            HStack(spacing: 12) {
                                PlaylistArtwork(playlist: playlist).frame(width: artworkEdge, height: artworkEdge)
                                VStack(alignment: .leading, spacing: 2) {
                                    Text(playlist.name).titleLines()
                                    Text(said("counted.songs", Int(playlist.songs))).font(.subheadline).foregroundStyle(.secondary)
                                }
                            }
                            .accessibilityElement(children: .combine)
                        }
                        .buttonStyle(.plain)
                        .disabled(asking)
                    }
                }
            }
            .navigationTitle("song.add-to-playlist")
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
        library.notice(said("playlist.picker.added", playlist))
        dismiss()
    }
}
