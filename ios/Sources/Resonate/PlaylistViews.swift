import ResonateNative
import SwiftUI

struct PlaylistEntry: Identifiable {
    let id: String
    let position: Int
    let track: Track

    static func of(_ rows: [Track]) -> [PlaylistEntry] {
        var seen: [UInt64: Int] = [:]
        return rows.enumerated().map { position, track in
            let occurrence = seen[track.id, default: 0]
            seen[track.id] = occurrence + 1
            return PlaylistEntry(id: "\(track.id)#\(occurrence)", position: position, track: track)
        }
    }
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
            ForEach(PlaylistEntry.of(rows)) { entry in
                PlaylistRow(entry: entry, count: rows.count, rows: rows, move: move, remove: remove)
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
                    PlaylistOptions(id: id, renaming: $renaming, deleting: $deleting)
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
        .sheet(isPresented: $choosingPlaylist) { PlaylistPicker(tracks: rows) }
        .sensoryFeedback(.success, trigger: queued)
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

    var body: some View {
        let features = library.features
        let sync = features.playlistSync.flatMap { $0.playlist == id ? $0 : nil }
        let devices = (features.syncStatus?.devices ?? []).filter { device in
            !device.thisDevice && (device.nearby || device.online && features.online)
        }
        Section {
            Button("Rename playlist", systemImage: "pencil") { renaming = true }
            Button("Undo last playlist edit", systemImage: "arrow.uturn.backward") { features.undoPlaylistEdit() }
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

struct PlaylistRow: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    let entry: PlaylistEntry
    let count: Int
    let rows: [Track]
    let move: (Int, Int) -> Void
    let remove: (Int) -> Void
    @State private var choosingPlaylist = false
    @State private var queued = 0

    var body: some View {
        let track = entry.track
        let position = entry.position
        let favourite = library.isFavourite(track)
        TrackRow(track: track, ownsMenus: false) {
            library.play(rows, at: position)
        }
        .swipeActions(edge: .trailing) {
            Button("Remove from playlist", systemImage: "minus.circle", role: .destructive) { remove(position) }
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
                if position > 0 {
                    Button("Move up", systemImage: "arrow.up") { move(position, position - 1) }
                }
                if position + 1 < count {
                    Button("Move down", systemImage: "arrow.down") { move(position, position + 1) }
                }
                Button("Song details", systemImage: "info.circle") { navigation.open(.inspector(track)) }
                Button("Remove from playlist", systemImage: "minus.circle", role: .destructive) { remove(position) }
            }
        }
        .sensoryFeedback(.success, trigger: queued)
        .sheet(isPresented: $choosingPlaylist) { PlaylistPicker(tracks: [track]) }
    }
}

struct AddSongsSheet: View {
    @Environment(Library.self) private var library
    @Environment(\.dismiss) private var dismiss
    let id: UInt64
    @State private var search = ""
    @State private var chosen: [UInt64] = []

    var body: some View {
        let folded = search.trimmingCharacters(in: .whitespacesAndNewlines).searchFolded
        let tracks = library.allTracks.filter { $0.matches(folded: folded) }
        NavigationStack {
            List(tracks, id: \.id) { track in
                Button {
                    if let at = chosen.firstIndex(of: track.id) {
                        chosen.remove(at: at)
                    } else {
                        chosen.append(track.id)
                    }
                } label: {
                    HStack {
                        CollectionRow(track: track, title: track.shownTitle, detail: track.shownArtist)
                        Spacer()
                        Image(systemName: chosen.contains(track.id) ? "checkmark.circle.fill" : "circle")
                            .foregroundStyle(.tint)
                    }
                }
                .buttonStyle(.plain)
                .accessibilityAddTraits(chosen.contains(track.id) ? .isSelected : [])
            }
            .overlay {
                if library.allTracks.isEmpty {
                    ContentUnavailableView("Add a music folder in Settings to start your library.", systemImage: "music.note")
                } else if tracks.isEmpty {
                    NoneMatch(kind: "songs", query: search, symbol: "magnifyingglass")
                }
            }
            .searchable(text: $search, prompt: "Find songs to add")
            .navigationTitle("Add songs")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button(role: .close) { dismiss() } }
                ToolbarItem(placement: .confirmationAction) {
                    Button(chosen.count == 1 ? "Add 1 song" : "Add \(chosen.count) songs", role: .confirm) {
                        let picked = chosen.compactMap { wanted in library.allTracks.first { $0.id == wanted } }
                        library.features.addToPlaylist(id, picked)
                        dismiss()
                    }
                    .disabled(chosen.isEmpty)
                }
            }
        }
    }
}

struct PlaylistPicker: View {
    @Environment(Library.self) private var library
    @Environment(\.dismiss) private var dismiss
    @ScaledMetric(relativeTo: .body) private var artworkEdge: CGFloat = 44
    let tracks: [Track]
    @State private var name = ""

    var body: some View {
        let trimmed = name.trimmingCharacters(in: .whitespacesAndNewlines)
        NavigationStack {
            List {
                Section {
                    TextField("New playlist name", text: $name)
                        .submitLabel(.done)
                        .onSubmit { create(trimmed) }
                    Button("Create and add", systemImage: "plus") { create(trimmed) }
                        .disabled(trimmed.isEmpty)
                }
                Section {
                    ForEach(library.features.playlists, id: \.id) { playlist in
                        Button {
                            library.features.addToPlaylist(playlist.id, tracks)
                            library.notice("Added to \(playlist.name)")
                            dismiss()
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
        guard !trimmed.isEmpty else { return }
        library.features.createPlaylist(trimmed, with: tracks)
        library.notice("Added to \(trimmed)")
        dismiss()
    }
}
