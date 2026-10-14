import ResonateNative
import SwiftUI

struct PlaylistView: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    @Environment(\.dismiss) private var dismiss
    let id: UInt64
    @State private var renaming = false
    @State private var deleting = false
    @State private var adding = false

    var body: some View {
        let id = id
        let features = library.features
        let playlist = features.playlists.first { $0.id == id }
        let rows = features.openPlaylist == id ? features.playlistRows : []
        let devices = features.syncStatus?.devices.filter { $0.online && !$0.thisDevice } ?? []
        List {
            CollectionHeader(title: playlist?.name ?? "Playlist", subtitle: "Playlist", tracks: rows)
            if let sync = features.playlistSync, sync.playlist == id, features.syncStatus?.standing != .off {
                Section("Your devices") {
                    Toggle("Keep its songs here", isOn: Binding(get: { sync.kept }, set: { features.keepSongs(id, $0) }))
                    if sync.missing > 0 {
                        ForEach(devices, id: \.name) { device in
                            Button("Fetch \(counted(Int(sync.missing), "missing song")) from \(device.name)") {
                                features.fetchMissing(id, from: device.name)
                            }
                        }
                    }
                }
            }
            ForEach(Array(rows.enumerated()), id: \.offset) { position, track in
                TrackRow(track: track, current: library.snapshot?.current?.id == track.id) {
                    library.play(rows, at: position)
                }
                .swipeActions {
                    Button("Remove from playlist", systemImage: "minus.circle", role: .destructive) {
                        remove(position, playlist)
                    }
                }
                .contextMenu {
                    if position > 0 {
                        Button("Move up", systemImage: "arrow.up") { move(position, to: position - 1, playlist) }
                    }
                    if position + 1 < rows.count {
                        Button("Move down", systemImage: "arrow.down") { move(position, to: position + 1, playlist) }
                    }
                    Button("Remove from playlist", systemImage: "minus.circle", role: .destructive) {
                        remove(position, playlist)
                    }
                    Button("Song details", systemImage: "info.circle") { navigation.open(.inspector(track)) }
                }
            }
            .onMove { from, to in
                guard let start = from.first else { return }
                move(start, to: to > start ? to - 1 : to, playlist)
            }
        }
        .listStyle(.plain)
        .background(BackdropView(track: rows.first))
        .scrollContentBackground(.hidden)
        .navigationTitle("Playlist")
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            Button("Add songs", systemImage: "plus") { adding = true }
            Menu("Playlist options", systemImage: "ellipsis.circle") {
                Button("Rename playlist", systemImage: "pencil") { renaming = true }
                Button("Undo last playlist edit", systemImage: "arrow.uturn.backward") {
                    features.editPlaylist { _ = try $0.undoPlaylist() }
                }
                Button("Delete playlist", systemImage: "trash", role: .destructive) { deleting = true }
            }
        }
        .onAppear {
            features.selectPlaylist(id)
            features.loadPlaylistSync(id)
        }
        .alert("Rename playlist", isPresented: $renaming) {
            PlaylistNameField(initial: playlist?.name ?? "") { name in
                guard let revision = playlist?.revision else { return }
                features.editPlaylist { try $0.renamePlaylist(id: id, revision: revision, name: name) }
            }
        }
        .confirmationDialog("Delete playlist", isPresented: $deleting) {
            Button("Delete playlist", role: .destructive) {
                guard let revision = playlist?.revision else { return }
                features.editPlaylist { try $0.deletePlaylist(id: id, revision: revision) }
                dismiss()
            }
        }
        .sheet(isPresented: $adding) { AddSongsSheet(id: id) }
    }

    private func move(_ from: Int, to: Int, _ playlist: MusicPlaylist?) {
        guard let revision = playlist?.revision else { return }
        let id = id
        library.features.editPlaylist {
            try $0.moveInPlaylist(id: id, revision: revision, from: UInt32(from), to: UInt32(to))
        }
    }

    private func remove(_ position: Int, _ playlist: MusicPlaylist?) {
        guard let revision = playlist?.revision else { return }
        let id = id
        library.features.editPlaylist {
            try $0.removeFromPlaylist(id: id, revision: revision, position: UInt32(position))
        }
    }
}

struct AddSongsSheet: View {
    @Environment(Library.self) private var library
    @Environment(\.dismiss) private var dismiss
    let id: UInt64
    @State private var search = ""
    @State private var chosen: [UInt64] = []

    var body: some View {
        let folded = search.lowercased()
        let tracks = library.allTracks.filter {
            folded.isEmpty || $0.shownTitle.lowercased().contains(folded) || $0.artist.lowercased().contains(folded) || $0.album.lowercased().contains(folded)
        }
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
            }
            .searchable(text: $search, prompt: "Find songs to add")
            .navigationTitle("Add songs")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
                ToolbarItem(placement: .confirmationAction) {
                    Button(chosen.count == 1 ? "Add 1 song" : "Add \(chosen.count) songs") {
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
    let tracks: [Track]
    @State private var naming = false

    var body: some View {
        NavigationStack {
            List {
                Button("Create playlist", systemImage: "plus") { naming = true }
                ForEach(library.features.playlists, id: \.id) { playlist in
                    Button {
                        library.features.addToPlaylist(playlist.id, tracks)
                        dismiss()
                    } label: {
                        HStack {
                            PlaylistArtwork(playlist: playlist).frame(width: 44, height: 44)
                            Text(playlist.name)
                            Spacer()
                            Text("\(playlist.songs)").foregroundStyle(.secondary)
                        }
                    }
                    .buttonStyle(.plain)
                }
            }
            .navigationTitle("Add to playlist")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
            }
            .onAppear { library.features.loadPlaylists() }
            .alert("Create and add", isPresented: $naming) {
                PlaylistNameField { name in
                    library.features.createPlaylist(name, with: tracks)
                    dismiss()
                }
            }
        }
        .presentationDetents([.medium, .large])
    }
}
