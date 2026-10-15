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
    @State private var importing = false

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
        ThemedList {
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
                    if library.filtering { library.setFilter(defaultTrackFilter()) }
                    navigation.songsGrouping = .songs
                    navigation.paths[.songs] = []
                    navigation.tab = .songs
                } label: {
                    AllSongsRow(count: tracks.count)
                }
                .buttonStyle(.plain)
                NavigationLink(value: Destination.suggestions) {
                    SuggestionsRow()
                }
                NavigationLink(value: Destination.statistics) {
                    StatisticsRow()
                }
            }
            if playlistsOnly || shelf == .all || shelf == .playlists {
                Section("Playlists") {
                    ForEach(playlists, id: \.id) { playlist in
                        NavigationLink(value: Destination.playlist(playlist.id)) {
                            PlaylistShelfRow(playlist: playlist)
                        }
                        .collectionActions(.playlist(playlist.id))
                    }
                    Button("Create playlist", systemImage: "plus") { naming = true }
                    Button("Import playlist file", systemImage: "square.and.arrow.down") { importing = true }
                    if playlists.isEmpty, folded.isEmpty {
                        Text("Make a playlist for a mood, a moment, or your next journey.").foregroundStyle(.secondary)
                    } else if playlists.isEmpty {
                        Text("No playlists match “\(asked)”").foregroundStyle(.secondary)
                    }
                }
            }
            if !playlistsOnly, shelf == .all, !albumHits.isEmpty {
                Section {
                    AlbumShelf(albums: Array(albumHits.prefix(Self.shelved)), query: asked) { navigation.open($0.destination) }
                } header: {
                    SeeAllHeader(title: "Albums", more: albumHits.count > Self.shelved) { shelf = .albums }
                }
            }
            if !playlistsOnly, shelf == .all, !artistHits.isEmpty {
                Section {
                    ArtistShelf(artists: Array(artistHits.prefix(Self.shelved)), query: asked) { navigation.open($0.destination) }
                } header: {
                    SeeAllHeader(title: "Artists", more: artistHits.count > Self.shelved) { shelf = .artists }
                }
            }
            if !playlistsOnly, shelf == .all, albumHits.isEmpty, artistHits.isEmpty, asked.isEmpty {
                Text("Add a music folder in Settings to see your albums and artists here.").foregroundStyle(.secondary)
            }
            if !playlistsOnly, shelf == .albums {
                Section("Albums") {
                    if albumHits.isEmpty, asked.isEmpty {
                        Text("Add a music folder in Settings to see your albums here.").foregroundStyle(.secondary)
                    } else if albumHits.isEmpty, answered {
                        Text("No albums match “\(asked)”").foregroundStyle(.secondary)
                    }
                    ForEach(albumHits, id: \.key) { album in
                        NavigationLink(value: album.destination) {
                            CollectionRow(
                                track: library.artworkTrack(album.artworkTrack),
                                title: lit(album.shownTitle, by: asked, in: .album),
                                detail: album.detail
                            )
                        }
                        .collectionActions(.album(album.key), artist: album.variousArtists ? nil : album.artist)
                    }
                }
            }
            if !playlistsOnly, shelf == .artists {
                Section("Artists") {
                    if artistHits.isEmpty, asked.isEmpty {
                        Text("Add a music folder in Settings to see your artists here.").foregroundStyle(.secondary)
                    } else if artistHits.isEmpty, answered {
                        Text("No artists match “\(asked)”").foregroundStyle(.secondary)
                    }
                    ForEach(artistHits, id: \.name) { artist in
                        NavigationLink(value: artist.destination) {
                            CollectionRow(
                                track: library.artworkTrack(artist.artworkTrack),
                                title: lit(artist.shownName, by: asked, in: .artist),
                                detail: artist.detail,
                                circle: true
                            )
                        }
                        .collectionActions(.artist(artist.name))
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
        .fileImporter(isPresented: $importing, allowedContentTypes: playlistFileTypes()) { result in
            switch result {
            case .success(let url): library.importPlaylistFile(url)
            case .failure(let failure): library.report(failure)
            }
        }
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

struct SuggestionsRow: View {
    @ScaledMetric(relativeTo: .body) private var artworkEdge: CGFloat = 56

    var body: some View {
        HStack(spacing: 14) {
            MosaicArtwork(covers: [], accents: MixAccents(from: .teal, to: .blue), mark: "sparkles", name: "", corner: 10)
                .frame(width: artworkEdge, height: artworkEdge)
            VStack(alignment: .leading, spacing: 2) {
                Text("Suggestions").font(.body.weight(.medium))
                Text("Mixes from your library").font(.subheadline).foregroundStyle(.secondary)
            }
        }
        .accessibilityElement(children: .combine)
    }
}

struct StatisticsRow: View {
    @ScaledMetric(relativeTo: .body) private var artworkEdge: CGFloat = 56

    var body: some View {
        HStack(spacing: 14) {
            MosaicArtwork(covers: [], accents: MixAccents(from: .mauve, to: .blue), mark: "chart.bar.fill", name: "", corner: 10)
                .frame(width: artworkEdge, height: artworkEdge)
            VStack(alignment: .leading, spacing: 2) {
                Text("Statistics").font(.body.weight(.medium))
                Text("Your listening and library in numbers").font(.subheadline).foregroundStyle(.secondary)
            }
        }
        .accessibilityElement(children: .combine)
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
        let name = playlist?.name ?? ""
        MosaicArtwork(covers: covers, accents: namedAccents(name: name), mark: "music.note.list", name: name, corner: corner)
            .task(id: "\(playlist?.id ?? 0):\(playlist?.revision ?? 0)") {
                guard let playlist, let native = try? await library.player() else { return }
                let id = playlist.id
                let rows = (try? await Work.aside { try native.playlistTracks(id: id, offset: 0, limit: 32) }) ?? []
                var seen = Set<String>()
                covers = Array(rows.filter { seen.insert(albumKey(album: $0.album, artist: $0.artist, albumArtist: $0.albumArtist) ?? "").inserted }.prefix(4))
            }
    }
}

func collectionSummary(_ tracks: [Track]) -> String {
    let minutes = tracks.reduce(UInt64(0)) { $0 + $1.durationMs } / 60_000
    let length = minutes >= 60 ? "\(minutes / 60) hr \(minutes % 60) min" : "\(minutes) min"
    return "\(counted(tracks.count, "song")) · \(length)"
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
    private static var widestActions: CGFloat { 440 }

    @Environment(Library.self) private var library
    @ScaledMetric(relativeTo: .title2) private var artworkEdge: CGFloat = 220
    let title: String
    let subtitle: String
    let tracks: [Track]
    var summary: String?
    var badge: String?
    var openSubtitle: (() -> Void)?
    @ViewBuilder let artwork: () -> Artwork
    @State private var acted = 0

    var body: some View {
        let edge = min(artworkEdge, Self.largestArtwork)
        VStack(spacing: 16) {
            artwork()
                .frame(width: edge, height: edge)
                .shadow(color: .black.opacity(0.25), radius: 18, y: 10)
            VStack(spacing: 6) {
                Text(title)
                    .font(.title2.weight(.bold))
                    .multilineTextAlignment(.center)
                    .accessibilityAddTraits(.isHeader)
                if !subtitle.isEmpty {
                    if let openSubtitle {
                        Button(subtitle, action: openSubtitle)
                            .font(.title3)
                            .foregroundStyle(.tint)
                            .buttonStyle(.plain)
                            .multilineTextAlignment(.center)
                    } else {
                        Text(subtitle).font(.subheadline).foregroundStyle(.secondary).multilineTextAlignment(.center)
                    }
                }
                Text(summary ?? collectionSummary(tracks))
                    .font(.footnote)
                    .foregroundStyle(.secondary)
                if let badge {
                    Text(badge)
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(.secondary)
                        .padding(.horizontal, 10)
                        .padding(.vertical, 4)
                        .background(.quaternary, in: .capsule)
                }
            }
            HStack(spacing: 12) {
                CollectionAction(title: "Play", symbol: "play.fill", prominent: true) {
                    acted += 1
                    library.play(tracks, at: 0)
                }
                CollectionAction(title: "Shuffle", symbol: "shuffle", prominent: false) {
                    acted += 1
                    library.shuffle(tracks)
                }
            }
            .frame(maxWidth: Self.widestActions)
            .disabled(tracks.isEmpty)
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 12)
        .sensoryFeedback(.success, trigger: acted)
        .listRowSeparator(.hidden)
        .listRowBackground(Color.clear)
    }
}

struct CollectionAction: View {
    @Environment(\.isEnabled) private var enabled
    @ScaledMetric(relativeTo: .headline) private var height: CGFloat = 50
    let title: String
    let symbol: String
    let prominent: Bool
    let action: () -> Void

    var body: some View {
        let ink = prominent ? AnyShapeStyle(.white) : AnyShapeStyle(.tint)
        Button(action: action) {
            HStack(spacing: 8) {
                Image(systemName: symbol)
                    .foregroundStyle(ink)
                    .accessibilityHidden(true)
                Text(title)
                    .foregroundStyle(ink)
            }
            .font(.headline)
            .frame(maxWidth: .infinity, minHeight: height)
            .contentShape(.capsule)
        }
        .buttonStyle(.plain)
        .accessibilityLabel(title)
        .glassEffect(prominent ? .regular.tint(Color.worn).interactive() : .regular.interactive(), in: .capsule)
        .opacity(enabled ? 1 : 0.5)
    }
}

extension CollectionHeader where Artwork == CollectionCover {
    init(
        title: String,
        subtitle: String,
        tracks: [Track],
        summary: String? = nil,
        badge: String? = nil,
        circle: Bool = false,
        openSubtitle: (() -> Void)? = nil
    ) {
        self.init(title: title, subtitle: subtitle, tracks: tracks, summary: summary, badge: badge, openSubtitle: openSubtitle) {
            CollectionCover(track: tracks.first, circle: circle)
        }
    }
}

struct CollectionMenu: View {
    @Environment(Navigation.self) private var navigation
    let tracks: [Track]
    var artist: String?
    var editsTags = false
    @State private var choosingPlaylist = false
    @State private var queued = 0

    var body: some View {
        Menu("More", systemImage: "ellipsis") {
            CollectionQueueActions(tracks: tracks, choosingPlaylist: $choosingPlaylist, queued: $queued)
            if let artist, !artist.isEmpty {
                Section {
                    Button("Go to artist", systemImage: "music.mic") { navigation.open(.artist(artist)) }
                }
            }
            if editsTags {
                Section {
                    Button("Edit album tags", systemImage: "pencil") { navigation.tagging = TagEditing(tracks: tracks) }
                }
            }
        }
        .disabled(tracks.isEmpty)
        .sensoryFeedback(.success, trigger: queued)
        .sheet(isPresented: $choosingPlaylist) { PlaylistPicker(tracks: tracks) }
    }
}

struct CollectionQueueActions: View {
    @Environment(Library.self) private var library
    let tracks: [Track]
    @Binding var choosingPlaylist: Bool
    @Binding var queued: Int

    var body: some View {
        Section {
            Button("Play next", systemImage: "text.line.first.and.arrowtriangle.forward") {
                queued += 1
                library.playNext(tracks)
            }
            Button("Add to queue", systemImage: "text.line.last.and.arrowtriangle.forward") {
                queued += 1
                library.addToQueue(tracks)
            }
        }
        Section {
            Button("Add to playlist", systemImage: "text.badge.plus") { choosingPlaylist = true }
        }
    }
}

struct AlbumDisc: Identifiable {
    let number: UInt32
    let tracks: [Track]

    var id: UInt32 { number }

    static func of(_ tracks: [Track]) -> [AlbumDisc] {
        var discs: [AlbumDisc] = []
        for track in tracks {
            let number = track.discNumber ?? 1
            if let last = discs.last, last.number == number {
                discs[discs.count - 1] = AlbumDisc(number: number, tracks: last.tracks + [track])
            } else {
                discs.append(AlbumDisc(number: number, tracks: [track]))
            }
        }
        return discs
    }
}

func sharedQuality(_ tracks: [Track]) -> String? {
    guard let quality = tracks.first?.quality, !quality.isEmpty, tracks.allSatisfy({ $0.quality == quality }) else { return nil }
    return quality
}

struct AlbumView: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    let key: String
    @State private var album: AlbumHit?
    @State private var tracks: [Track] = []
    @State private var read = false

    var body: some View {
        let shownTitle = album?.shownTitle ?? ""
        let artist = album?.artist ?? ""
        let discs = AlbumDisc.of(tracks)
        List {
            CollectionHeader(
                title: shownTitle,
                subtitle: album?.shownArtist ?? "",
                tracks: tracks,
                summary: ([album?.year.map(String.init)].compactMap { $0 } + [collectionSummary(tracks)]).joined(separator: " · "),
                badge: sharedQuality(tracks),
                openSubtitle: artist.isEmpty || album?.variousArtists == true ? nil : { navigation.open(.artist(artist)) }
            )
            if read, tracks.isEmpty {
                Text("No songs from this album are in your library yet.").foregroundStyle(.secondary)
            }
            ForEach(discs) { disc in
                Section {
                    ForEach(disc.tracks, id: \.id) { track in
                        TrackRow(track: track, numbered: track.trackNumber, albumArtist: artist) {
                            library.play(tracks, selected: track)
                        }
                        .listRowBackground(Color.clear)
                    }
                } header: {
                    if discs.count > 1 {
                        Text("Disc \(disc.number)")
                    }
                }
            }
        }
        .listStyle(.plain)
        .background(BackdropView(track: tracks.first, moving: false))
        .scrollContentBackground(.hidden)
        .navigationTitle(shownTitle)
        .navigationBarTitleDisplayMode(.inline)
        .toolbar { CollectionMenu(tracks: tracks, artist: album?.variousArtists == true ? nil : artist, editsTags: true) }
        .task(id: "\(key):\(library.collectionGeneration)") { await load() }
    }

    private func load() async {
        guard let native = try? await library.player() else { return }
        let key = key
        guard let (found, rows) = try? await Work.aside({ (try native.album(key: key), try native.albumTracks(key: key)) }) else {
            return
        }
        album = found
        tracks = rows
        read = true
    }
}

struct ArtistView: View {
    private static let topSongs: UInt32 = 5

    @Environment(Library.self) private var library
    @ScaledMetric(relativeTo: .subheadline) private var albumEdge: CGFloat = 140
    let name: String
    @State private var albums: [AlbumHit] = []
    @State private var tracks: [Track] = []
    @State private var top: [Track] = []
    @State private var seeingAll = false
    @State private var read = false

    var body: some View {
        let shownName = name.isEmpty ? "Unknown artist" : name
        List {
            CollectionHeader(
                title: shownName,
                subtitle: "Artist",
                tracks: tracks,
                summary: "\(counted(albums.count, "album")) · \(counted(tracks.count, "song"))",
                circle: true
            )
            if read, tracks.isEmpty {
                Text("No music by this artist is in your library yet.").foregroundStyle(.secondary)
            }
            if !albums.isEmpty {
                Section("Albums") {
                    ScrollView(.horizontal, showsIndicators: false) {
                        LazyHStack(spacing: 14) {
                            ForEach(albums, id: \.key) { album in
                                NavigationLink(value: album.destination) {
                                    VStack(alignment: .leading) {
                                        ArtworkView(track: library.artworkTrack(album.artworkTrack), corner: 10)
                                            .frame(width: albumEdge, height: albumEdge)
                                        Text(album.shownTitle)
                                            .font(.subheadline.weight(.medium))
                                            .lineLimit(1)
                                        Text(album.year.map(String.init) ?? counted(Int(album.songs), "song"))
                                            .font(.caption)
                                            .foregroundStyle(.secondary)
                                    }
                                    .frame(width: albumEdge)
                                    .accessibilityElement(children: .combine)
                                }
                                .buttonStyle(.plain)
                                .collectionActions(.album(album.key), artist: album.variousArtists ? nil : album.artist)
                            }
                        }
                        .scrollTargetLayout()
                    }
                    .scrollTargetBehavior(.viewAligned)
                    .listRowInsets(EdgeInsets(top: 8, leading: 16, bottom: 8, trailing: 16))
                }
                .listRowBackground(Color.clear)
            }
            if !tracks.isEmpty {
                let shown = seeingAll || top.isEmpty ? tracks : top
                Section {
                    ForEach(shown, id: \.id) { track in
                        TrackRow(track: track, showsAlbum: true) {
                            library.play(shown, selected: track)
                        }
                    }
                } header: {
                    SeeAllHeader(title: seeingAll ? "Songs" : "Top songs", more: !seeingAll && tracks.count > top.count) {
                        seeingAll = true
                    }
                }
                .listRowBackground(Color.clear)
            }
        }
        .listStyle(.plain)
        .background(BackdropView(track: tracks.first, moving: false))
        .scrollContentBackground(.hidden)
        .navigationTitle(shownName)
        .navigationBarTitleDisplayMode(.inline)
        .toolbar { CollectionMenu(tracks: tracks) }
        .task(id: "\(name):\(library.collectionGeneration)") { await load() }
    }

    private func load() async {
        guard let native = try? await library.player() else { return }
        let name = name
        let most = Self.topSongs
        guard let (found, rows, played) = try? await Work.aside({
            (try native.albumsBy(name: name), try native.artistTracks(name: name), try native.mostPlayedBy(name: name, limit: most))
        }) else {
            return
        }
        albums = found
        tracks = rows
        top = played
        read = true
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
            .themedRows()
        }
        .listStyle(.plain)
        .searchable(text: $search, prompt: "Search favourites")
        .navigationTitle("Favourites")
        .toolbar { CollectionMenu(tracks: favourites) }
        .navigationBarTitleDisplayMode(.inline)
    }
}

enum CollectionSource: Sendable {
    case album(String)
    case artist(String)
    case playlist(UInt64)

    nonisolated func tracks(_ native: MobilePlayer) throws -> [Track] {
        switch self {
        case .album(let key): try native.albumTracks(key: key)
        case .artist(let name): try native.artistTracks(name: name)
        case .playlist(let id): try native.playlistTracks(id: id, offset: 0, limit: Library.page)
        }
    }
}

struct PickedTracks: Identifiable {
    let id = UUID()
    let tracks: [Track]
}

struct CollectionActions: ViewModifier {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    let source: CollectionSource
    var artist: String?
    @State private var picked: PickedTracks?
    @State private var acted = 0

    func body(content: Content) -> some View {
        content
            .contextMenu {
                Section {
                    Button("Play", systemImage: "play.fill") { act { library.play($0, at: 0) } }
                    Button("Shuffle", systemImage: "shuffle") { act { library.shuffle($0) } }
                }
                Section {
                    Button("Play next", systemImage: "text.line.first.and.arrowtriangle.forward") { act { library.playNext($0) } }
                    Button("Add to queue", systemImage: "text.line.last.and.arrowtriangle.forward") { act { library.addToQueue($0) } }
                }
                Section {
                    Button("Add to playlist", systemImage: "text.badge.plus") { act { picked = PickedTracks(tracks: $0) } }
                    if let artist, !artist.isEmpty {
                        Button("Go to artist", systemImage: "music.mic") { navigation.open(.artist(artist)) }
                    }
                }
            }
            .swipeActions(edge: .leading, allowsFullSwipe: true) {
                Button("Play next", systemImage: "text.line.first.and.arrowtriangle.forward") { act { library.playNext($0) } }
                    .tint(.indigo)
                Button("Add to queue", systemImage: "text.line.last.and.arrowtriangle.forward") { act { library.addToQueue($0) } }
                    .tint(.orange)
            }
            .sheet(item: $picked) { PlaylistPicker(tracks: $0.tracks) }
            .sensoryFeedback(.success, trigger: acted)
    }

    private func act(_ action: @escaping @MainActor ([Track]) -> Void) {
        let source = source
        Task {
            guard let native = try? await library.player(),
                let tracks = try? await Work.aside({ try source.tracks(native) }),
                !tracks.isEmpty
            else { return }
            acted += 1
            action(tracks)
        }
    }
}

extension View {
    func collectionActions(_ source: CollectionSource, artist: String? = nil) -> some View {
        modifier(CollectionActions(source: source, artist: artist))
    }
}
