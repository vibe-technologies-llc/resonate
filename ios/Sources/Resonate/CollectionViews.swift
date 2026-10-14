import ResonateNative
import SwiftUI

enum Shelf: String, CaseIterable, Identifiable {
    case all = "All"
    case playlists = "Playlists"
    case albums = "Albums"
    case artists = "Artists"

    var id: String { rawValue }
}

struct LibraryView: View {
    private static let shelved = 12

    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    var playlistsOnly = false
    @State private var shelf = Shelf.all
    @State private var playlistSearch = ""
    @State private var naming = false

    var body: some View {
        @Bindable var shelfSearch = library.shelfSearch
        let asked = playlistsOnly ? playlistSearch.trimmingCharacters(in: .whitespacesAndNewlines) : shelfSearch.query
        let folded = asked.searchFolded
        let answered = !asked.isEmpty && shelfSearch.hasAnswer
        let tracks = library.allTracks
        let favourites = tracks.filter(\.favourite)
        let playlists = library.features.playlists.filter { folded.isEmpty || $0.name.searchFolded.contains(folded) }
        let albumHits = asked.isEmpty ? library.albums : shelfSearch.found.albums
        let artistHits = asked.isEmpty ? library.artists : shelfSearch.found.artists
        let matchedNothing = !playlistsOnly && answered && playlists.isEmpty && albumHits.isEmpty && artistHits.isEmpty && !"favourites".contains(folded)
        List {
            if matchedNothing {
                ContentUnavailableView.search(text: asked)
                    .listRowSeparator(.hidden)
            }
            if !playlistsOnly {
                Picker("Show", selection: $shelf) {
                    ForEach(Shelf.allCases) { Text($0.rawValue).tag($0) }
                }
                .pickerStyle(.segmented)
                .listRowSeparator(.hidden)
            }
            if !playlistsOnly, shelf == .all || shelf == .playlists, folded.isEmpty || "favourites".contains(folded) {
                NavigationLink(value: Destination.favourites) {
                    HStack(spacing: 14) {
                        Image(systemName: "heart.fill")
                            .font(.title2)
                            .foregroundStyle(.white)
                            .frame(width: 56, height: 56)
                            .background(LinearGradient(colors: [.pink, .purple], startPoint: .topLeading, endPoint: .bottomTrailing), in: .rect(cornerRadius: 10))
                        VStack(alignment: .leading) {
                            Text("Favourites").font(.body.weight(.medium))
                            Text(counted(favourites.count, "song")).font(.subheadline).foregroundStyle(.secondary)
                        }
                    }
                }
            }
            if playlistsOnly || shelf == .all || shelf == .playlists {
                Section("Playlists") {
                    ForEach(playlists, id: \.id) { playlist in
                        NavigationLink(value: Destination.playlist(playlist.id)) {
                            HStack(spacing: 14) {
                                PlaylistArtwork(playlist: playlist).frame(width: 56, height: 56)
                                VStack(alignment: .leading) {
                                    Text(playlist.name).font(.body.weight(.medium)).lineLimit(1)
                                    Text(counted(Int(playlist.songs), "song")).font(.subheadline).foregroundStyle(.secondary)
                                }
                            }
                        }
                    }
                    Button("Create playlist", systemImage: "plus") { naming = true }
                    if playlists.isEmpty, folded.isEmpty {
                        Text("Make a playlist for a mood, a moment, or your next journey.").foregroundStyle(.secondary)
                    } else if playlists.isEmpty {
                        Text("No playlists match “\(asked)”").foregroundStyle(.secondary)
                    }
                }
            }
            if !playlistsOnly, shelf == .all || shelf == .albums {
                Section("Albums") {
                    if albumHits.isEmpty, asked.isEmpty {
                        Text("Add a music folder in Settings to see your albums here.").foregroundStyle(.secondary)
                    } else if albumHits.isEmpty, answered {
                        Text("No albums match “\(asked)”").foregroundStyle(.secondary)
                    }
                    ForEach(shelf == .all ? Array(albumHits.prefix(Self.shelved)) : albumHits, id: \.key) { album in
                        NavigationLink(value: album.destination) {
                            FoundCollectionRow(
                                track: library.artworkTrack(album.artworkTrack),
                                title: lit(album.shownTitle, by: asked, in: .album),
                                detail: album.detail,
                                disclosed: false
                            )
                        }
                    }
                    if shelf == .all, albumHits.count > Self.shelved {
                        Button("All albums") { shelf = .albums }
                    }
                }
            }
            if !playlistsOnly, shelf == .all || shelf == .artists {
                Section("Artists") {
                    if artistHits.isEmpty, asked.isEmpty {
                        Text("Add a music folder in Settings to see your artists here.").foregroundStyle(.secondary)
                    } else if artistHits.isEmpty, answered {
                        Text("No artists match “\(asked)”").foregroundStyle(.secondary)
                    }
                    ForEach(shelf == .all ? Array(artistHits.prefix(Self.shelved)) : artistHits, id: \.name) { artist in
                        NavigationLink(value: artist.destination) {
                            FoundCollectionRow(
                                track: library.artworkTrack(artist.artworkTrack),
                                title: lit(artist.shownName, by: asked, in: .artist),
                                detail: artist.detail,
                                circle: true,
                                disclosed: false
                            )
                        }
                    }
                    if shelf == .all, artistHits.count > Self.shelved {
                        Button("All artists") { shelf = .artists }
                    }
                }
            }
        }
        .listStyle(.insetGrouped)
        .searchable(text: playlistsOnly ? $playlistSearch : $shelfSearch.text, prompt: playlistsOnly ? "Search playlists" : "Search your library")
        .navigationTitle(playlistsOnly ? "Playlists" : "Your library")
        .toolbar {
            LibraryToolbar()
            if library.features.playlists.isEmpty == false {
                ToolbarItem(placement: .topBarLeading) {
                    Button("Undo last playlist edit", systemImage: "arrow.uturn.backward") {
                        library.features.editPlaylist { _ = try $0.undoPlaylist() }
                    }
                }
            }
        }
        .onAppear { library.features.loadPlaylists() }
        .alert("Create playlist", isPresented: $naming) {
            PlaylistNameField { library.features.createPlaylist($0) }
        }
    }
}

struct PlaylistNameField: View {
    var initial = ""
    let save: (String) -> Void
    @State private var name = ""

    var body: some View {
        TextField("Playlist name", text: $name)
            .onAppear { name = initial }
        Button("Cancel", role: .cancel) {}
        Button("Save") {
            let trimmed = name.trimmingCharacters(in: .whitespacesAndNewlines)
            if !trimmed.isEmpty { save(trimmed) }
        }
    }
}

struct PlaylistArtwork: View {
    @Environment(Library.self) private var library
    let playlist: MusicPlaylist
    @State private var covers: [Track] = []

    var body: some View {
        Group {
            if covers.count >= 4 {
                Grid(horizontalSpacing: 0, verticalSpacing: 0) {
                    GridRow {
                        ArtworkView(track: covers[0], corner: 0)
                        ArtworkView(track: covers[1], corner: 0)
                    }
                    GridRow {
                        ArtworkView(track: covers[2], corner: 0)
                        ArtworkView(track: covers[3], corner: 0)
                    }
                }
                .clipShape(.rect(cornerRadius: 10))
            } else {
                ArtworkView(track: covers.first, corner: 10)
            }
        }
        .task(id: "\(playlist.id):\(playlist.revision)") {
            guard let native = try? await library.player() else { return }
            let id = playlist.id
            let rows = (try? await Work.aside { try native.playlistTracks(id: id, offset: 0, limit: 32) }) ?? []
            var seen = Set<String>()
            covers = Array(rows.filter { seen.insert("\($0.album)\u{1F}\($0.groupingArtist)").inserted }.prefix(4))
        }
    }
}

struct CollectionHeader: View {
    @Environment(Library.self) private var library
    let title: String
    let subtitle: String
    let tracks: [Track]
    var circle = false

    var body: some View {
        VStack(spacing: 14) {
            ArtworkView(track: tracks.first, corner: 20, circle: circle)
                .frame(width: 220, height: 220)
                .shadow(color: .black.opacity(0.25), radius: 18, y: 10)
            VStack(spacing: 4) {
                Text(title).font(.title2.weight(.bold)).multilineTextAlignment(.center)
                Text(subtitle).font(.subheadline).foregroundStyle(.secondary)
                Text("\(counted(tracks.count, "song")) · \(clock(tracks.reduce(0) { $0 + $1.durationMs }))")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }
            GlassEffectContainer {
                HStack(spacing: 12) {
                    Button("Play", systemImage: "play.fill") { library.play(tracks, at: 0) }
                        .buttonStyle(.glassProminent)
                    Button("Shuffle", systemImage: "shuffle") {
                        library.play(tracks, at: Int.random(in: 0..<max(tracks.count, 1)), shuffle: true)
                    }
                    .buttonStyle(.glass)
                }
                .disabled(tracks.isEmpty)
            }
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 12)
        .listRowSeparator(.hidden)
        .listRowBackground(Color.clear)
    }
}

struct AlbumView: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    let title: String
    let artist: String

    var body: some View {
        let tracks = library.allTracks
            .filter { $0.album == title && $0.groupingArtist == artist }
            .sorted { ($0.discNumber ?? 0, $0.trackNumber ?? 0, $0.shownTitle) < ($1.discNumber ?? 0, $1.trackNumber ?? 0, $1.shownTitle) }
        List {
            CollectionHeader(title: title.isEmpty ? "Unknown album" : title, subtitle: artist, tracks: tracks)
            if tracks.isEmpty {
                Text("No songs from this album are in your library yet.").foregroundStyle(.secondary)
            }
            ForEach(tracks, id: \.id) { track in
                TrackRow(track: track, current: library.snapshot?.current?.id == track.id, numbered: track.trackNumber) {
                    library.play(tracks, selected: track)
                }
            }
        }
        .listStyle(.plain)
        .background(BackdropView(track: tracks.first))
        .scrollContentBackground(.hidden)
        .navigationTitle("Album")
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            if !artist.isEmpty {
                Button("Artist", systemImage: "music.mic") { navigation.open(.artist(artist)) }
            }
        }
    }
}

struct ArtistView: View {
    @Environment(Library.self) private var library
    let name: String

    var body: some View {
        let tracks = library.allTracks.filter { $0.artist == name || $0.albumArtist == name }
        let records = albums(of: tracks)
        List {
            CollectionHeader(title: name.isEmpty ? "Unknown artist" : name, subtitle: "Artist", tracks: tracks, circle: true)
            if tracks.isEmpty {
                Text("No music by this artist is in your library yet.").foregroundStyle(.secondary)
            }
            if !records.isEmpty {
                Section("Albums") {
                    ScrollView(.horizontal, showsIndicators: false) {
                        HStack(spacing: 14) {
                            ForEach(records) { group in
                                NavigationLink(value: Destination.album(title: group.title, artist: group.artist)) {
                                    VStack(alignment: .leading) {
                                        ArtworkView(track: group.tracks.first).frame(width: 140, height: 140)
                                        Text(group.title.isEmpty ? "Unknown album" : group.title).font(.subheadline.weight(.medium)).lineLimit(1)
                                        Text(counted(group.tracks.count, "song")).font(.caption).foregroundStyle(.secondary)
                                    }
                                    .frame(width: 140)
                                }
                                .buttonStyle(.plain)
                            }
                        }
                    }
                }
            }
            Section("Songs") {
                ForEach(tracks, id: \.id) { track in
                    TrackRow(track: track, current: library.snapshot?.current?.id == track.id) {
                        library.play(tracks, selected: track)
                    }
                }
            }
        }
        .listStyle(.plain)
        .background(BackdropView(track: tracks.first))
        .scrollContentBackground(.hidden)
        .navigationTitle("Artist")
        .navigationBarTitleDisplayMode(.inline)
    }
}

struct FavouritesView: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    @State private var search = ""

    var body: some View {
        let folded = search.trimmingCharacters(in: .whitespacesAndNewlines).searchFolded
        let tracks = library.allTracks.filter { $0.favourite && $0.matches(folded: folded) }
        List {
            CollectionHeader(title: "Favourites", subtitle: "Your favourites", tracks: tracks)
            if tracks.isEmpty, folded.isEmpty {
                VStack(spacing: 12) {
                    Text("Tap a heart to keep a song here.").foregroundStyle(.secondary)
                    Button("Browse songs") { navigation.tab = .songs }.buttonStyle(.glass)
                }
                .frame(maxWidth: .infinity)
            } else if tracks.isEmpty {
                NoneMatch(kind: "songs", query: search, symbol: "magnifyingglass")
            }
            ForEach(tracks, id: \.id) { track in
                TrackRow(track: track, current: library.snapshot?.current?.id == track.id) {
                    library.play(tracks, selected: track)
                }
            }
        }
        .listStyle(.plain)
        .searchable(text: $search, prompt: "Search favourites")
        .navigationTitle("Favourites")
        .navigationBarTitleDisplayMode(.inline)
    }
}
