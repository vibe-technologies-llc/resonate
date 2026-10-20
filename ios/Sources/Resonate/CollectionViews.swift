import ResonateLogic
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
        let playlists = library.features.playlists.filter { folded.isEmpty || library.features.foldedName(of: $0).contains(folded) }
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
                    FavouritesCountRow()
                }
            }
            if !playlistsOnly, shelf == .all, folded.isEmpty {
                NavigationLink(value: Destination.songs) {
                    SongsCountRow()
                }
                .simultaneousGesture(TapGesture().onEnded {
                    if library.filtering { library.setFilter(Library.everything) }
                    navigation.songsGrouping = .songs
                })
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
                    AlbumShelf(albums: Array(albumHits.prefix(Self.shelved)), query: asked)
                } header: {
                    SeeAllHeader(title: "Albums", more: albumHits.count > Self.shelved) { shelf = .albums }
                }
            }
            if !playlistsOnly, shelf == .all, !artistHits.isEmpty {
                Section {
                    ArtistShelf(artists: Array(artistHits.prefix(Self.shelved)), query: asked)
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
    static let corner = ShelfTile.corner

    var body: some View {
        ShelfTile(symbol: "heart.fill", from: .pink, to: .purple)
    }
}

struct ShelfTile: View {
    static let corner: CGFloat = 10

    let symbol: String
    let from: Color
    let to: Color

    var body: some View {
        Image(systemName: symbol)
            .font(.title2)
            .foregroundStyle(.white)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(
                LinearGradient(colors: [from, to], startPoint: .topLeading, endPoint: .bottomTrailing),
                in: .rect(cornerRadius: Self.corner)
            )
            .accessibilityHidden(true)
    }
}

struct AllSongsRow: View {
    @ScaledMetric(relativeTo: .body) private var artworkEdge: CGFloat = 56
    let count: Int

    var body: some View {
        HStack(spacing: 14) {
            ShelfTile(symbol: "music.note", from: .orange, to: .red)
                .frame(width: artworkEdge, height: artworkEdge)
            VStack(alignment: .leading, spacing: 2) {
                Text("All songs").font(.body.weight(.medium))
                Text(counted(count, "song")).font(.subheadline).foregroundStyle(.secondary)
            }
        }
        .accessibilityElement(children: .combine)
    }
}

struct SuggestionsRow: View {
    @ScaledMetric(relativeTo: .body) private var artworkEdge: CGFloat = 56

    var body: some View {
        HStack(spacing: 14) {
            ShelfTile(symbol: "sparkles", from: .teal, to: .blue)
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
            ShelfTile(symbol: "chart.bar.fill", from: .indigo, to: .blue)
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
    private static let rowsSampled: UInt32 = 32
    private static let coversShown = 4

    @Environment(Library.self) private var library
    let playlist: MusicPlaylist?
    var corner: CGFloat = 10
    @State private var covers: [Track]?

    var body: some View {
        let name = playlist?.name ?? ""
        let held = playlist.flatMap { PlaylistCovers.held(for: $0) }
        MosaicArtwork(covers: covers ?? held ?? [], accents: namedAccents(name: name), mark: "music.note.list", name: name, corner: corner)
            .task(id: "\(playlist?.id ?? 0):\(playlist?.revision ?? 0)") {
                guard let playlist else { return }
                if let held = PlaylistCovers.held(for: playlist) {
                    covers = held
                    return
                }
                guard let native = try? await library.player() else { return }
                let id = playlist.id
                let sampled = Self.rowsSampled
                let shown = Self.coversShown
                let found = try? await Work.aside { try Self.distinctAlbums(of: native.playlistTracks(id: id, offset: 0, limit: sampled), keeping: shown) }
                guard !Task.isCancelled else { return }

                if let found { PlaylistCovers.keep(found, for: playlist) }
                covers = found ?? []
            }
    }

    private nonisolated static func distinctAlbums(of rows: [Track], keeping count: Int) -> [Track] {
        var seen = Set<String>()
        return Array(rows.filter { seen.insert(albumKey(album: $0.album, artist: $0.artist, albumArtist: $0.albumArtist) ?? "").inserted }.prefix(count))
    }
}

@MainActor
enum PlaylistCovers {
    private struct Held {
        let revision: UInt64
        let covers: [Track]
    }

    private static var kept: [UInt64: Held] = [:]

    static func held(for playlist: MusicPlaylist) -> [Track]? {
        guard let held = kept[playlist.id], held.revision == playlist.revision else { return nil }
        return held.covers
    }

    static func keep(_ covers: [Track], for playlist: MusicPlaylist) {
        kept[playlist.id] = Held(revision: playlist.revision, covers: covers)
    }
}

func collectionSummary(_ tracks: [Track]) -> String {
    let minutes = tracks.reduce(UInt64(0)) { $0 + $1.durationMs } / 60_000
    let length = minutes >= 60 ? "\(minutes / 60) hr \(minutes % 60) min" : "\(minutes) min"
    return "\(counted(tracks.count, "song")) · \(length)"
}

enum CoverOutline {
    case rounded(CGFloat)
    case circle

    func path(in rect: CGRect) -> Path {
        switch self {
        case .rounded(let corner): RoundedRectangle(cornerRadius: corner, style: .continuous).path(in: rect)
        case .circle: Circle().path(in: rect)
        }
    }
}

struct HeaderShadow: View {
    private static let blur: CGFloat = 18
    private static let drop: CGFloat = 10
    private static let darkness = 0.25
    private static let reach = blur * 2 + drop
    private static let drawnAtScale: CGFloat = 1

    let outline: CoverOutline

    var body: some View {
        Canvas { context, size in
            let cover = CGRect(origin: .zero, size: size).insetBy(dx: Self.reach, dy: Self.reach)
            guard cover.width > 0, cover.height > 0 else { return }

            context.addFilter(.shadow(color: .black.opacity(Self.darkness), radius: Self.blur, y: Self.drop, options: .shadowOnly))
            context.fill(outline.path(in: cover), with: .color(.black))
        }
        .environment(\.displayScale, Self.drawnAtScale)
        .padding(-Self.reach)
        .allowsHitTesting(false)
        .accessibilityHidden(true)
    }
}

struct CollectionCover: View {
    static let corner: CGFloat = 20

    let track: Track?
    var circle = false

    var body: some View {
        ArtworkView(track: track, corner: Self.corner, circle: circle)
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
    var outline = CoverOutline.rounded(CollectionCover.corner)
    @ViewBuilder let artwork: () -> Artwork
    @State private var acted = 0

    var body: some View {
        let edge = min(artworkEdge, Self.largestArtwork)
        VStack(spacing: 16) {
            artwork()
                .frame(width: edge, height: edge)
                .background { HeaderShadow(outline: outline) }
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
        let ink = prominent ? AnyShapeStyle(Color.wornInk) : AnyShapeStyle(.tint)
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
        self.init(
            title: title,
            subtitle: subtitle,
            tracks: tracks,
            summary: summary,
            badge: badge,
            openSubtitle: openSubtitle,
            outline: circle ? .circle : .rounded(CollectionCover.corner)
        ) {
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
                    Button("Edit album tags", systemImage: "pencil") { navigation.editTags(tracks) }
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

struct ReadFailure: View {
    let text: String
    let retry: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Label(text, systemImage: "exclamationmark.triangle")
                .foregroundStyle(.secondary)
            Button("Try again", action: retry)
        }
    }
}

struct AlbumView: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    @Environment(\.dismiss) private var dismiss
    @State private var key: String
    @State private var album: AlbumHit?
    @State private var tracks: [Track] = []
    @State private var read = false
    @State private var failure: String?

    init(key: String) {
        _key = State(initialValue: key)
    }

    var body: some View {
        let shownTitle = album?.shownTitle ?? ""
        let artist = album?.artist ?? ""
        let credit = AlbumCredit(artist)
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
            if let failure {
                ReadFailure(text: failure) { Task { await load() } }
            } else if read, tracks.isEmpty {
                Text("No songs from this album are in your library yet.").foregroundStyle(.secondary)
            }
            ForEach(discs) { disc in
                Section {
                    ForEach(disc.tracks, id: \.id) { track in
                        TrackRow(track: track, numbered: track.trackNumber, albumArtist: credit) {
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
        .task(id: "\(key):\(library.collectionGeneration):\(library.catalogEpoch)") { await load() }
    }

    private func load() async {
        guard let native = try? await library.player() else { return }
        let key = key
        let found: AlbumHit?
        let rows: [Track]
        do {
            (found, rows) = try await Work.aside { (try native.album(key: key), try native.albumTracks(key: key)) }
        } catch {
            failure = explained(error)
            return
        }
        failure = nil
        if rows.isEmpty, let shown = tracks.compactMap({ library.artworkTracks[$0.id] }).first {
            let moved = albumKey(album: shown.album, artist: shown.artist, albumArtist: shown.albumArtist)
            guard let moved else { return dismiss() }
            guard moved == key else {
                self.key = moved
                return
            }
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
    @State private var name: String
    @State private var albums: [AlbumHit] = []
    @State private var tracks: [Track] = []
    @State private var top: [Track] = []
    @State private var seeingAll = false
    @State private var read = false
    @State private var failure: String?

    init(name: String) {
        _name = State(initialValue: name)
    }

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
            if let failure {
                ReadFailure(text: failure) { Task { await load() } }
            } else if read, tracks.isEmpty {
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
        .task(id: "\(name):\(library.collectionGeneration):\(library.catalogEpoch)") { await load() }
    }

    private func load() async {
        guard let native = try? await library.player() else { return }
        let name = name
        let most = Self.topSongs
        let found: [AlbumHit]
        let rows: [Track]
        let played: [Track]
        do {
            (found, rows, played) = try await Work.aside {
                (try native.albumsBy(name: name), try native.artistTracks(name: name), try native.mostPlayedBy(name: name, limit: most))
            }
        } catch {
            failure = explained(error)
            return
        }
        failure = nil
        if rows.isEmpty, let shown = tracks.compactMap({ library.artworkTracks[$0.id] }).first {
            let followed = await Self.artist(holding: shown, native)
            if let followed, followed != name {
                self.name = followed
                return
            }
        }
        albums = found
        tracks = rows
        top = played
        read = true
    }
}

extension ArtistView {
    fileprivate static func artist(holding shown: Track, _ native: MobilePlayer) async -> String? {
        for candidate in [shown.artist, shown.albumArtist] {
            let holding = try? await Work.aside { try native.artistTracks(name: candidate) }
            if holding?.contains(where: { $0.id == shown.id }) == true { return candidate }
        }
        return nil
    }
}

struct FavouritesView: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    @State private var search = ""
    @State private var keyed = SongKeys.empty
    @State private var keyedAt = -1
    @State private var found: [Track] = []
    @State private var foundFor = ""

    private struct Asked: Equatable {
        let epoch: Int
        let folded: String
    }

    var body: some View {
        let folded = search.trimmingCharacters(in: .whitespacesAndNewlines).searchFolded
        let favourites = library.favouriteTracks
        let shown = folded.isEmpty ? favourites : found
        List {
            CollectionHeader(title: "Favourites", subtitle: "", tracks: favourites, outline: .rounded(FavouritesArtwork.corner)) {
                FavouritesArtwork()
            }
            if favourites.isEmpty {
                VStack(spacing: 12) {
                    Text("Keep the songs you love together. Tap a heart on any song or on the player to add it here.")
                        .multilineTextAlignment(.center)
                        .foregroundStyle(.secondary)
                    Button("Browse songs") { navigation.showSongs() }.buttonStyle(.glass)
                }
                .frame(maxWidth: .infinity)
                .listRowSeparator(.hidden)
            } else if shown.isEmpty, folded.isEmpty || foundFor == folded {
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
        .task(id: Asked(epoch: library.favouriteEpoch, folded: folded)) {
            guard !folded.isEmpty else { return }
            try? await Task.sleep(for: .milliseconds(150))
            guard !Task.isCancelled else { return }
            let epoch = library.favouriteEpoch
            if keyedAt != epoch {
                let tracks = library.favouriteTracks
                keyed = (try? await Work.aside { SongKeys.of(tracks) }) ?? keyed
                guard !Task.isCancelled else { return }
                keyedAt = epoch
            }
            let keys = keyed
            let matching = (try? await Work.aside { keys.matching(folded) }) ?? []
            guard !Task.isCancelled else { return }
            found = matching
            foundFor = folded
        }
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
        case .playlist(let id): try Library.everyPage { try native.playlistTracks(id: id, offset: $0, limit: Library.page) }
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
            do {
                let native = try await library.player()
                let tracks = try await Work.aside { try source.tracks(native) }
                guard !tracks.isEmpty else { return }
                acted += 1
                action(tracks)
            } catch {
                library.report(error)
            }
        }
    }
}

extension View {
    func collectionActions(_ source: CollectionSource, artist: String? = nil) -> some View {
        modifier(CollectionActions(source: source, artist: artist))
    }
}

private struct FavouritesCountRow: View {
    @Environment(Library.self) private var library

    var body: some View {
        PinnedFavouritesRow(count: library.favouriteTracks.count)
    }
}

private struct SongsCountRow: View {
    @Environment(Library.self) private var library

    var body: some View {
        AllSongsRow(count: library.catalogCount)
    }
}
