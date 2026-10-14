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
            if playlistsOnly || shelf == .all || shelf == .playlists, folded.isEmpty || "favourites".contains(folded) {
                NavigationLink(value: Destination.favourites) {
                    PinnedFavouritesRow(count: favourites.count)
                }
            }
            if !playlistsOnly, shelf == .all, folded.isEmpty {
                Button {
                    navigation.tab = .songs
                } label: {
                    AllSongsRow(count: tracks.count)
                }
                .buttonStyle(.plain)
            }
            if playlistsOnly || shelf == .all || shelf == .playlists {
                Section("Playlists") {
                    ForEach(playlists, id: \.id) { playlist in
                        NavigationLink(value: Destination.playlist(playlist.id)) {
                            PlaylistShelfRow(playlist: playlist)
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
        .refreshable {
            library.rescan()
            library.features.loadPlaylists()
        }
        .toolbar {
            if !library.features.playlists.isEmpty {
                ToolbarItem(placement: .topBarLeading) {
                    Button("Undo last playlist edit", systemImage: "arrow.uturn.backward") {
                        library.features.undoPlaylistEdit()
                    }
                }
            }
            LibraryToolbar()
        }
        .onAppear { library.features.loadPlaylists() }
        .alert("Create playlist", isPresented: $naming) {
            PlaylistNameField(confirm: "Create") { library.features.createPlaylist($0) }
        }
    }
}

struct PinnedFavouritesRow: View {
    @ScaledMetric(relativeTo: .body) private var artworkEdge: CGFloat = 56
    let count: Int

    var body: some View {
        HStack(spacing: 14) {
            FavouritesArtwork()
                .frame(width: artworkEdge, height: artworkEdge)
            VStack(alignment: .leading, spacing: 2) {
                Text("Favourites").font(.body.weight(.medium))
                Label("Pinned · \(counted(count, "song"))", systemImage: "pin.fill")
                    .labelStyle(PinnedLabelStyle())
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            }
        }
        .accessibilityElement(children: .combine)
    }
}

struct PinnedLabelStyle: LabelStyle {
    func makeBody(configuration: Configuration) -> some View {
        HStack(spacing: 4) {
            configuration.icon.font(.caption2).foregroundStyle(.tint)
            configuration.title
        }
    }
}

struct FavouritesArtwork: View {
    var body: some View {
        Image(systemName: "heart.fill")
            .font(.title2)
            .foregroundStyle(.white)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(
                LinearGradient(colors: [.pink, .purple], startPoint: .topLeading, endPoint: .bottomTrailing),
                in: .rect(cornerRadius: 10)
            )
            .accessibilityHidden(true)
    }
}

struct AllSongsRow: View {
    @ScaledMetric(relativeTo: .body) private var artworkEdge: CGFloat = 56
    let count: Int

    var body: some View {
        HStack(spacing: 14) {
            Image(systemName: "music.note.list")
                .font(.title2)
                .foregroundStyle(.tint)
                .frame(width: artworkEdge, height: artworkEdge)
                .background(.quaternary, in: .rect(cornerRadius: 10))
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 2) {
                Text("All songs").font(.body.weight(.medium))
                Text(counted(count, "song")).font(.subheadline).foregroundStyle(.secondary)
            }
            Spacer(minLength: 0)
        }
        .contentShape(.rect)
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(.isButton)
    }
}

struct PlaylistShelfRow: View {
    @ScaledMetric(relativeTo: .body) private var artworkEdge: CGFloat = 56
    let playlist: MusicPlaylist

    var body: some View {
        HStack(spacing: 14) {
            PlaylistArtwork(playlist: playlist).frame(width: artworkEdge, height: artworkEdge)
            VStack(alignment: .leading, spacing: 2) {
                Text(playlist.name).font(.body.weight(.medium)).lineLimit(1)
                Text("Playlist · \(counted(Int(playlist.songs), "song"))").font(.subheadline).foregroundStyle(.secondary)
            }
        }
        .accessibilityElement(children: .combine)
    }
}

struct PlaylistNameField: View {
    var initial = ""
    var confirm = "Save"
    let save: (String) -> Void
    @State private var name = ""

    var body: some View {
        let trimmed = name.trimmingCharacters(in: .whitespacesAndNewlines)
        TextField("Playlist name", text: $name)
            .onAppear { name = initial }
        Button("Cancel", role: .cancel) {}
        Button(confirm) { save(trimmed) }
            .disabled(trimmed.isEmpty)
    }
}

struct PlaylistArtwork: View {
    @Environment(Library.self) private var library
    let playlist: MusicPlaylist?
    var corner: CGFloat = 10
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
                .clipShape(.rect(cornerRadius: corner, style: .continuous))
            } else if let first = covers.first {
                ArtworkView(track: first, corner: corner)
            } else {
                Image(systemName: "music.note.list")
                    .font(.title2)
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .background(.quaternary, in: .rect(cornerRadius: corner, style: .continuous))
            }
        }
        .aspectRatio(1, contentMode: .fit)
        .accessibilityHidden(true)
        .task(id: "\(playlist?.id ?? 0):\(playlist?.revision ?? 0)") {
            guard let playlist, let native = try? await library.player() else { return }
            let id = playlist.id
            let rows = (try? await Work.aside { try native.playlistTracks(id: id, offset: 0, limit: 32) }) ?? []
            var seen = Set<String>()
            covers = Array(rows.filter { seen.insert("\($0.album)\u{1F}\($0.groupingArtist)").inserted }.prefix(4))
        }
    }
}

func collectionSummary(_ tracks: [Track]) -> String {
    let minutes = tracks.reduce(UInt64(0)) { $0 + $1.durationMs } / 60_000
    let length = minutes >= 60 ? "\(minutes / 60) hr \(minutes % 60) min" : "\(minutes) min"
    return "\(counted(tracks.count, "song")) · \(length)"
}

func albumSongs(in tracks: [Track], title: String, artist: String) -> [Track] {
    tracks
        .filter { $0.album == title && $0.groupingArtist == artist }
        .sorted { first, second in
            (first.discNumber ?? 1, first.trackNumber ?? .max, first.shownTitle)
                < (second.discNumber ?? 1, second.trackNumber ?? .max, second.shownTitle)
        }
}

func artistSongs(in tracks: [Track], named name: String) -> [Track] {
    guard !name.isEmpty else { return tracks.filter { $0.artist.isEmpty } }
    return tracks.filter { track in
        track.artist.caseInsensitiveCompare(name) == .orderedSame
            || track.albumArtist.caseInsensitiveCompare(name) == .orderedSame
    }
}

struct ShelvedAlbum: Identifiable {
    let title: String
    let artist: String
    let tracks: [Track]

    var id: String { "\(title.count):\(title)\(artist)" }

    static func of(_ tracks: [Track]) -> [ShelvedAlbum] {
        var order: [String] = []
        var grouped: [String: ShelvedAlbum] = [:]
        for track in tracks {
            let album = ShelvedAlbum(title: track.album, artist: track.groupingArtist, tracks: [track])
            if let kept = grouped[album.id] {
                grouped[album.id] = ShelvedAlbum(title: kept.title, artist: kept.artist, tracks: kept.tracks + [track])
            } else {
                order.append(album.id)
                grouped[album.id] = album
            }
        }
        return order.compactMap { grouped[$0] }
    }
}

struct CollectionCover: View {
    let track: Track?
    var circle = false

    var body: some View {
        ArtworkView(track: track, corner: 20, circle: circle)
    }
}

struct CollectionHeader<Artwork: View>: View {
    private static var largestArtwork: CGFloat { 300 }

    @Environment(Library.self) private var library
    @ScaledMetric(relativeTo: .title2) private var artworkEdge: CGFloat = 220
    let title: String
    let subtitle: String
    let tracks: [Track]
    var summary: String?
    @ViewBuilder let artwork: () -> Artwork
    @State private var acted = 0

    var body: some View {
        let edge = min(artworkEdge, Self.largestArtwork)
        VStack(spacing: 14) {
            artwork()
                .frame(width: edge, height: edge)
                .shadow(color: .black.opacity(0.25), radius: 18, y: 10)
            VStack(spacing: 4) {
                Text(title).font(.title2.weight(.bold)).multilineTextAlignment(.center)
                if !subtitle.isEmpty {
                    Text(subtitle).font(.subheadline).foregroundStyle(.secondary).multilineTextAlignment(.center)
                }
                Text(summary ?? collectionSummary(tracks))
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }
            .accessibilityElement(children: .combine)
            .accessibilityAddTraits(.isHeader)
            GlassEffectContainer(spacing: 12) {
                HStack(spacing: 12) {
                    Button("Play", systemImage: "play.fill") {
                        acted += 1
                        library.play(tracks, at: 0)
                    }
                    .buttonStyle(.glassProminent)
                    Button("Shuffle", systemImage: "shuffle") {
                        acted += 1
                        library.shuffle(tracks)
                    }
                    .buttonStyle(.glass)
                    Button("Play next", systemImage: "text.line.first.and.arrowtriangle.forward") {
                        acted += 1
                        library.playNext(tracks)
                    }
                    .labelStyle(.iconOnly)
                    .buttonStyle(.glass)
                    Button("Add to queue", systemImage: "text.line.last.and.arrowtriangle.forward") {
                        acted += 1
                        library.addToQueue(tracks)
                    }
                    .labelStyle(.iconOnly)
                    .buttonStyle(.glass)
                }
                .disabled(tracks.isEmpty)
            }
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 12)
        .sensoryFeedback(.success, trigger: acted)
        .listRowSeparator(.hidden)
        .listRowBackground(Color.clear)
    }
}

extension CollectionHeader where Artwork == CollectionCover {
    init(title: String, subtitle: String, tracks: [Track], summary: String? = nil, circle: Bool = false) {
        self.init(title: title, subtitle: subtitle, tracks: tracks, summary: summary) {
            CollectionCover(track: tracks.first, circle: circle)
        }
    }
}

struct AlbumView: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    let title: String
    let artist: String

    var body: some View {
        let tracks = albumSongs(in: library.allTracks, title: title, artist: artist)
        let shownTitle = title.isEmpty ? "Unknown album" : title
        let year = tracks.lazy.map { String($0.date.prefix(4)) }.first { $0.count == 4 && Int($0) != nil }
        List {
            CollectionHeader(
                title: shownTitle,
                subtitle: artist.isEmpty ? "Unknown artist" : artist,
                tracks: tracks,
                summary: ([year].compactMap { $0 } + [collectionSummary(tracks)]).joined(separator: " · ")
            )
            if tracks.isEmpty {
                Text("No songs from this album are in your library yet.").foregroundStyle(.secondary)
            }
            ForEach(tracks, id: \.id) { track in
                TrackRow(track: track, numbered: track.trackNumber) {
                    library.play(tracks, selected: track)
                }
                .listRowBackground(Color.clear)
            }
        }
        .listStyle(.plain)
        .background(BackdropView(track: tracks.first, moving: false))
        .scrollContentBackground(.hidden)
        .navigationTitle(shownTitle)
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            if !artist.isEmpty {
                Button("Go to artist", systemImage: "music.mic") { navigation.open(.artist(artist)) }
            }
        }
    }
}

struct ArtistView: View {
    @Environment(Library.self) private var library
    @ScaledMetric(relativeTo: .subheadline) private var albumEdge: CGFloat = 140
    let name: String

    var body: some View {
        let tracks = artistSongs(in: library.allTracks, named: name)
        let records = ShelvedAlbum.of(tracks)
        let shownName = name.isEmpty ? "Unknown artist" : name
        List {
            CollectionHeader(
                title: shownName,
                subtitle: "Artist",
                tracks: tracks,
                summary: "\(counted(records.count, "album")) · \(counted(tracks.count, "song"))",
                circle: true
            )
            if tracks.isEmpty {
                Text("No music by this artist is in your library yet.").foregroundStyle(.secondary)
            }
            if !records.isEmpty {
                Section("Albums") {
                    ScrollView(.horizontal, showsIndicators: false) {
                        LazyHStack(spacing: 14) {
                            ForEach(records) { record in
                                NavigationLink(value: Destination.album(title: record.title, artist: record.artist)) {
                                    VStack(alignment: .leading) {
                                        ArtworkView(track: record.tracks.first).frame(width: albumEdge, height: albumEdge)
                                        Text(record.title.isEmpty ? "Unknown album" : record.title)
                                            .font(.subheadline.weight(.medium))
                                            .lineLimit(1)
                                        Text(counted(record.tracks.count, "song")).font(.caption).foregroundStyle(.secondary)
                                    }
                                    .frame(width: albumEdge)
                                    .accessibilityElement(children: .combine)
                                }
                                .buttonStyle(.plain)
                            }
                        }
                        .scrollTargetLayout()
                    }
                    .scrollTargetBehavior(.viewAligned)
                    .listRowInsets(EdgeInsets(top: 8, leading: 16, bottom: 8, trailing: 16))
                }
                .listRowBackground(Color.clear)
            }
            Section("Songs") {
                ForEach(tracks, id: \.id) { track in
                    TrackRow(track: track) {
                        library.play(tracks, selected: track)
                    }
                }
            }
            .listRowBackground(Color.clear)
        }
        .listStyle(.plain)
        .background(BackdropView(track: tracks.first, moving: false))
        .scrollContentBackground(.hidden)
        .navigationTitle(shownName)
        .navigationBarTitleDisplayMode(.inline)
    }
}

struct FavouritesView: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    @State private var search = ""

    var body: some View {
        let folded = search.trimmingCharacters(in: .whitespacesAndNewlines).searchFolded
        let favourites = library.allTracks.filter(\.favourite)
        let shown = favourites.filter { $0.matches(folded: folded) }
        List {
            CollectionHeader(title: "Favourites", subtitle: "", tracks: favourites) {
                FavouritesArtwork()
            }
            if favourites.isEmpty {
                VStack(spacing: 12) {
                    Text("Keep the songs you love together. Tap a heart on any song or on the player to add it here.")
                        .multilineTextAlignment(.center)
                        .foregroundStyle(.secondary)
                    Button("Browse songs") { navigation.tab = .songs }.buttonStyle(.glass)
                }
                .frame(maxWidth: .infinity)
                .listRowSeparator(.hidden)
            } else if shown.isEmpty {
                NoneMatch(kind: "favourites", query: search, symbol: "magnifyingglass")
            }
            ForEach(shown, id: \.id) { track in
                TrackRow(track: track) {
                    library.play(shown, selected: track)
                }
            }
        }
        .listStyle(.plain)
        .searchable(text: $search, prompt: "Search favourites")
        .navigationTitle("Favourites")
        .navigationBarTitleDisplayMode(.inline)
    }
}
