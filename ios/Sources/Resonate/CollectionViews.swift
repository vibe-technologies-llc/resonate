import ResonateLogic
import ResonateNative
import SwiftUI

enum LibraryPage {
    case home
    case playlists
    case albums
    case artists

    var title: String {
        switch self {
        case .home: said("library.title")
        case .playlists: said("library.playlists")
        case .albums: said("library.albums")
        case .artists: said("library.artists")
        }
    }

    var searchPrompt: String {
        switch self {
        case .home: said("library.search.prompt")
        case .playlists: said("library.playlists.search.prompt")
        case .albums: said("library.albums.search.prompt")
        case .artists: said("library.artists.search.prompt")
        }
    }
}

struct SeeAllPageHeader: View {
    private static let reachBeyondTheLine: CGFloat = 12

    let title: String
    let more: Bool
    let page: Destination

    var body: some View {
        HStack {
            Text(title)
            Spacer()
            if more {
                NavigationLink(value: page) {
                    Text("library.see-all")
                        .font(.subheadline)
                        .textCase(nil)
                        .frame(minWidth: 44)
                        .padding(.vertical, Self.reachBeyondTheLine)
                        .contentShape(.rect)
                        .padding(.vertical, -Self.reachBeyondTheLine)
                }
                .accessibilityLabel(said("library.see-all.titled", title.lowercased()))
            }
        }
    }
}

struct LibraryView: View {
    private static let shelved = 12

    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    var page = LibraryPage.home
    @State private var playlistSearch = ""
    @State private var naming = false
    @State private var importing = false
    @State private var unfollowing: String?

    var body: some View {
        @Bindable var shelfSearch = library.shelfSearch
        let asked = page == .playlists ? playlistSearch.trimmingCharacters(in: .whitespacesAndNewlines) : shelfSearch.query
        let folded = asked.searchFolded
        let answered = !asked.isEmpty && shelfSearch.hasAnswer
        let lighting = asked.isEmpty ? "" : shelfSearch.answered
        let playlists = library.features.playlists.filter { folded.isEmpty || library.features.foldedName(of: $0).contains(folded) }
        let albumHits = asked.isEmpty ? library.albums : shelfSearch.found.albums
        let artistHits = asked.isEmpty ? library.artists : shelfSearch.found.artists
        let matchedNothing = page == .home && answered && playlists.isEmpty && albumHits.isEmpty && artistHits.isEmpty && !"favourites".contains(folded)
        ThemedList {
            if matchedNothing {
                ContentUnavailableView.search(text: asked)
                    .listRowSeparator(.hidden)
            }
            if page == .playlists, folded.isEmpty || "favourites".contains(folded) {
                NavigationLink(value: Destination.favourites) {
                    FavouritesCountRow()
                }
            }
            if page == .home, folded.isEmpty || "favourites".contains(folded) {
                Section {
                    ShortcutCards {
                        NavigationLink(value: Destination.favourites) {
                            FavouritesCard()
                        }
                        .buttonStyle(.plain)
                        if folded.isEmpty {
                            Button {
                                if library.filtering { library.setFilter(Library.everything) }
                                navigation.songsGrouping = .songs
                                navigation.open(.songs)
                            } label: {
                                AllSongsCard()
                            }
                            .buttonStyle(.plain)
                        }
                    }
                }
            }
            if page == .home, folded.isEmpty {
                NavigationLink(value: Destination.wanted) {
                    ToolRow(title: said("tools.saved-for-later"), detail: said("tools.saved-for-later.detail"), symbol: "bookmark")
                }
            }
            if page == .home, folded.isEmpty || !playlists.isEmpty {
                Section {
                    if playlists.isEmpty {
                        Text("library.playlists.none").foregroundStyle(.secondary)
                    } else {
                        PlaylistShelf(playlists: Array(playlists.prefix(Self.shelved)))
                    }
                } header: {
                    SeeAllPageHeader(title: said("library.playlists"), more: true, page: .playlists)
                }
            }
            if page == .home, !albumHits.isEmpty {
                Section {
                    AlbumShelf(albums: Array(albumHits.prefix(Self.shelved)), query: lighting)
                } header: {
                    SeeAllPageHeader(title: said("library.albums"), more: true, page: .albums)
                }
            }
            if page == .home, !artistHits.isEmpty {
                Section {
                    ArtistShelf(artists: Array(artistHits.prefix(Self.shelved)), query: lighting)
                } header: {
                    SeeAllPageHeader(title: said("library.artists"), more: true, page: .artists)
                }
            }
            if page == .home, albumHits.isEmpty, artistHits.isEmpty, asked.isEmpty {
                Text("library.empty.albums-and-artists").foregroundStyle(.secondary)
            }
            if page == .playlists {
                Section("library.playlists") {
                    ForEach(playlists, id: \.id) { playlist in
                        NavigationLink(value: Destination.playlist(playlist.id)) {
                            PlaylistShelfRow(playlist: playlist)
                        }
                        .collectionActions(.playlist(playlist.id))
                    }
                    if playlists.isEmpty {
                        Text(folded.isEmpty ? said("library.playlists.none") : said("library.playlists.none-matching", asked)).foregroundStyle(.secondary)
                    }
                }
            }
            if page == .albums {
                Section {
                    if albumHits.isEmpty, asked.isEmpty {
                        Text("library.empty.albums").foregroundStyle(.secondary)
                    } else if albumHits.isEmpty, answered {
                        Text(said("library.albums.none-matching", asked)).foregroundStyle(.secondary)
                    }
                    ForEach(albumHits, id: \.key) { album in
                        NavigationLink(value: album.destination) {
                            CollectionRow(
                                track: library.artworkTrack(album.artworkTrack),
                                title: lit(album.shownTitle, by: lighting, in: .album),
                                detail: album.detail
                            )
                        }
                        .collectionActions(.album(album.key), artist: album.variousArtists ? nil : album.artist)
                    }
                }
            }
            if page == .artists {
                Section {
                    if artistHits.isEmpty, asked.isEmpty {
                        Text("library.empty.artists").foregroundStyle(.secondary)
                    } else if artistHits.isEmpty, answered {
                        Text(said("library.artists.none-matching", asked)).foregroundStyle(.secondary)
                    }
                    ForEach(artistHits, id: \.name) { artist in
                        NavigationLink(value: artist.destination) {
                            CollectionRow(
                                track: library.artworkTrack(artist.artworkTrack),
                                title: lit(artist.shownName, by: lighting, in: .artist),
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
        .searchable(text: page == .playlists ? $playlistSearch : $shelfSearch.text, prompt: page.searchPrompt)
        .navigationTitle(page.title)
        .refreshable {
            library.rescan()
            library.features.loadPlaylists()
        }
        .toolbar {
            if page == .playlists {
                if !library.features.playlists.isEmpty {
                    ToolbarItem(placement: .topBarLeading) {
                        Button("playlists.undo", systemImage: "arrow.uturn.backward") {
                            library.features.undoPlaylistEdit()
                        }
                    }
                }
                ToolbarItem(placement: .topBarTrailing) {
                    Menu("playlists.new", systemImage: "plus") {
                        Button("playlists.create", systemImage: "plus") { naming = true }
                        Button("playlists.import-file", systemImage: "square.and.arrow.down") { importing = true }
                        Button("playlists.paste-link", systemImage: "doc.on.clipboard") {
                            library.features.together.accept(UIPasteboard.general.string ?? "")
                        }
                    }
                }
                if !library.features.followedProfiles.isEmpty {
                    ToolbarItem(placement: .topBarTrailing) {
                        Menu("playlists.followed-profiles", systemImage: "person.2") {
                            Section(FollowedProfileWords.stopFollowingHeading) {
                                ForEach(library.features.followedProfiles, id: \.link) { profile in
                                    Button(FollowedProfileWords.followed(profile)) { unfollowing = profile.link }
                                }
                            }
                        }
                    }
                }
                ToolbarSpacer(.fixed, placement: .topBarTrailing)
            }
            if page == .albums || page == .artists {
                ToolbarItem(placement: .topBarTrailing) {
                    CollectionOrderMenu(grouping: page == .albums ? .albums : .artists)
                }
                ToolbarSpacer(.fixed, placement: .topBarTrailing)
            }
            LibraryToolbar()
        }
        .onAppear { library.features.loadPlaylists() }
        .fileImporter(isPresented: $importing, allowedContentTypes: PlaylistFileTypes.offered) { result in
            switch result {
            case .success(let url): library.importPlaylistFile(url)
            case .failure(let failure): library.report(failure)
            }
        }
        .confirmationDialog(
            unfollowing.map { link in
                FollowedProfileWords.stopFollowing(
                    library.features.followedProfiles.first { $0.link == link }?.name ?? link
                )
            } ?? "",
            isPresented: Binding(get: { unfollowing != nil }, set: { if !$0 { unfollowing = nil } }),
            titleVisibility: .visible,
            presenting: unfollowing
        ) { link in
            Button("playlists.stop-following", role: .destructive) { library.features.unfollowProfile(link) }
        } message: { _ in
            Text(FollowedProfileWords.stoppingExplained)
        }
        .alert("playlists.create", isPresented: $naming) {
            PlaylistNameField(confirm: said("common.create")) { library.features.createPlaylist($0) }
        }
    }
}

struct FavouritesRow: View {
    @ScaledMetric(relativeTo: .body) private var artworkEdge: CGFloat = 56
    let count: Int

    var body: some View {
        HStack(spacing: 14) {
            FavouritesArtwork()
                .frame(width: artworkEdge, height: artworkEdge)
            VStack(alignment: .leading, spacing: 2) {
                Text("favourites.title").font(.body.weight(.medium))
                Text(said("counted.songs", count)).font(.subheadline).foregroundStyle(.secondary)
            }
        }
        .contentShape(.rect)
        .accessibilityElement(children: .combine)
    }
}

struct FavouritesArtwork: View {
    var corner = ShelfTile.rowCorner

    var body: some View {
        ShelfTile(symbol: "heart.fill", corner: corner)
    }
}

struct ShelfTile: View {
    static let rowCorner: CGFloat = 10
    private static let markShare: CGFloat = 0.34

    let symbol: String
    var corner = ShelfTile.rowCorner

    var body: some View {
        Rectangle()
            .fill(.quaternary)
            .overlay {
                GeometryReader { proxy in
                    let side = min(proxy.size.width, proxy.size.height) * Self.markShare
                    Image(systemName: symbol)
                        .resizable()
                        .scaledToFit()
                        .foregroundStyle(.tint)
                        .frame(width: side, height: side)
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                }
            }
            .clipShape(.rect(cornerRadius: corner, style: .continuous))
            .accessibilityHidden(true)
            .allowsHitTesting(false)
    }
}

struct ShortcutCards<Cards: View>: View {
    @Environment(\.dynamicTypeSize) private var typeSize
    @ViewBuilder let cards: Cards

    var body: some View {
        let layout = typeSize.isAccessibilitySize
            ? AnyLayout(VStackLayout(spacing: ShortcutCard.spacing))
            : AnyLayout(HStackLayout(spacing: ShortcutCard.spacing))
        ScrollView(.horizontal) {
            layout { cards }
        }
        .scrollDisabled(true)
        .scrollIndicators(.hidden)
        .listRowInsets(EdgeInsets(top: ShortcutCard.belowTheTitle, leading: 0, bottom: 0, trailing: 0))
        .listRowBackground(Color.clear)
        .listRowSeparator(.hidden)
    }
}

struct ShortcutCard: View {
    static let corner: CGFloat = 20
    static let spacing: CGFloat = 12
    static let belowTheTitle: CGFloat = 16

    @Environment(\.dynamicTypeSize) private var typeSize
    @Environment(\.isEnabled) private var enabled
    @ScaledMetric(relativeTo: .headline) private var height: CGFloat = 92
    let title: String
    let detail: String
    let symbol: String

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: Self.corner, style: .continuous)
        VStack(alignment: .leading, spacing: 2) {
            Image(systemName: symbol)
                .font(.title3.weight(.medium))
                .foregroundStyle(.tint)
                .accessibilityHidden(true)
            Spacer(minLength: 10)
            Text(title)
                .font(.headline)
                .titleLines()
            Text(detail)
                .font(.subheadline)
                .foregroundStyle(.secondary)
                .titleLines()
        }
        .padding(14)
        .frame(maxWidth: .infinity, minHeight: height, alignment: .leading)
        .background(Color.cardGround, in: shape)
        .contentShape(shape)
        .opacity(enabled ? 1 : 0.5)
        .containerRelativeFrame(.horizontal, count: typeSize.isAccessibilitySize ? 1 : 2, span: 1, spacing: Self.spacing)
        .accessibilityElement(children: .combine)
    }
}

struct FavouritesCard: View {
    @Environment(Library.self) private var library

    var body: some View {
        ShortcutCard(
            title: said("favourites.title"),
            detail: said("counted.songs", library.favouriteTracks.count),
            symbol: "heart.fill"
        )
    }
}

private struct AllSongsCard: View {
    @Environment(Library.self) private var library

    var body: some View {
        ShortcutCard(
            title: said("library.all-songs"),
            detail: said("counted.songs", library.catalogCount),
            symbol: "music.note"
        )
    }
}

struct PlaylistShelfRow: View {
    @Environment(Library.self) private var library
    @ScaledMetric(relativeTo: .body) private var artworkEdge: CGFloat = 56
    let playlist: MusicPlaylist

    var body: some View {
        let kind = SharedPlaylistWords.kind(shared: library.features.together.shared(playlist.id)?.mayEdit, followed: playlist.followed) ?? said("playlist.kind")
        HStack(spacing: 14) {
            PlaylistArtwork(playlist: playlist).frame(width: artworkEdge, height: artworkEdge)
            VStack(alignment: .leading, spacing: 2) {
                Text(playlist.name).font(.body.weight(.medium)).titleLines()
                Text("\(kind) · \(said("counted.songs", Int(playlist.songs)))").font(.subheadline).foregroundStyle(.secondary)
            }
        }
        .accessibilityElement(children: .combine)
    }
}

struct PlaylistShelf: View {
    @ScaledMetric(relativeTo: .subheadline) private var edge: CGFloat = 140
    let playlists: [MusicPlaylist]

    var body: some View {
        Shelf(spacing: 14) {
            ForEach(playlists, id: \.id) { playlist in
                NavigationLink(value: Destination.playlist(playlist.id)) {
                    VStack(alignment: .leading, spacing: 4) {
                        PlaylistArtwork(playlist: playlist)
                            .frame(width: edge, height: edge)
                        Text(playlist.name)
                            .font(.subheadline.weight(.medium))
                            .titleLines()
                        Text(said("counted.songs", Int(playlist.songs)))
                            .font(.caption)
                            .foregroundStyle(.secondary)
                            .titleLines()
                    }
                    .frame(width: edge, alignment: .leading)
                    .contentShape(.rect)
                }
                .buttonStyle(.plain)
                .collectionActions(.playlist(playlist.id))
            }
        }
        .listRowInsets(EdgeInsets(top: 8, leading: 0, bottom: 8, trailing: 0))
        .listRowSeparator(.hidden)
    }
}

struct PlaylistNameField: View {
    var initial = ""
    var confirm = said("common.save")
    let save: (String) -> Void
    @State private var name = ""

    var body: some View {
        let trimmed = name.trimmingCharacters(in: .whitespacesAndNewlines)
        TextField("playlists.name", text: $name)
            .onAppear { name = initial }
        Button("common.cancel", role: .cancel) {}
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
        MosaicArtwork(covers: covers ?? held ?? [], mark: "music.note.list", name: name, corner: corner)
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
                let found = try? await Work.onScreen { try Self.distinctAlbums(of: native.playlistTracks(id: id, offset: 0, limit: sampled), keeping: shown) }
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
    let length = minutes >= 60 ? said("collection.length.hours", Int(minutes / 60), Int(minutes % 60)) : said("collection.length.minutes", Int(minutes))
    return "\(said("counted.songs", tracks.count)) · \(length)"
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
        Canvas(rendersAsynchronously: true) { context, size in
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
                CollectionAction(title: said("common.play"), symbol: "play.fill", prominent: true) {
                    acted += 1
                    library.play(tracks, at: 0)
                }
                CollectionAction(title: said("common.shuffle"), symbol: "shuffle", prominent: false) {
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
    @Environment(Library.self) private var library
    let tracks: [Track]
    var artist: String?
    var editsTags = false
    var keeping: SavedSearch?
    var keepingAs = ""
    @State private var choosingPlaylist = false
    @State private var queued = 0

    var body: some View {
        Menu("common.more", systemImage: "ellipsis") {
            CollectionQueueActions(tracks: tracks, choosingPlaylist: $choosingPlaylist, queued: $queued)
            if let artist, !artist.isEmpty {
                Section {
                    Button("song.go-to-artist", systemImage: "music.mic") { navigation.open(.artist(artist)) }
                }
            }
            if editsTags {
                Section {
                    Button("album.edit-tags", systemImage: "pencil") { navigation.editTags(tracks) }
                }
            }
            if let keeping {
                Section {
                    if library.features.playlists.contains(where: { $0.search == keeping }) {
                        Button("mix.kept-as-playlist", systemImage: "checkmark") {}.disabled(true)
                    } else {
                        Button("mix.keep-as-playlist", systemImage: "bookmark") { keep(keeping) }
                    }
                }
            }
        }
        .disabled(tracks.isEmpty)
        .sensoryFeedback(.success, trigger: queued)
        .sheet(isPresented: $choosingPlaylist) { PlaylistPicker(tracks: tracks).sheetSized() }
    }
}

extension CollectionMenu {
    private func keep(_ search: SavedSearch) {
        let name = keepingAs
        library.features.keepSearchAsPlaylist(name, search: search) { problem in
            if let problem {
                library.fail(problem)
            } else {
                library.notice(said("mix.kept", name))
            }
        }
    }
}

struct CollectionQueueActions: View {
    @Environment(Library.self) private var library
    let tracks: [Track]
    @Binding var choosingPlaylist: Bool
    @Binding var queued: Int

    var body: some View {
        Section {
            Button("song.play-next", systemImage: "text.line.first.and.arrowtriangle.forward") {
                queued += 1
                library.playNext(tracks)
            }
            Button("song.add-to-queue", systemImage: "text.line.last.and.arrowtriangle.forward") {
                queued += 1
                library.addToQueue(tracks)
            }
        }
        Section {
            Button("song.add-to-playlist", systemImage: "text.badge.plus") { choosingPlaylist = true }
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
            Button("common.try-again", action: retry)
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
    @State private var notHeld: [SearchSong] = []
    @State private var read = false
    @State private var failure: String?

    init(key: String) {
        _key = State(initialValue: key)
    }

    var body: some View {
        let shownTitle = album?.shownTitle ?? ""
        let artist = album?.artist ?? ""
        let credit = AlbumCredit(artist)
        let discs = AlbumDisc.of(tracks, notHeld: notHeld)
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
                Text("album.empty").foregroundStyle(.secondary)
            }
            ForEach(discs) { disc in
                Section {
                    ForEach(disc.rows) { row in
                        switch row {
                        case .held(let track):
                            TrackRow(track: track, numbered: track.trackNumber, albumArtist: credit) {
                                library.play(tracks, selected: track)
                            }
                            .listRowBackground(Color.clear)
                        case .notHeld(let song):
                            NotHeldSongRow(song: song)
                                .listRowBackground(Color.clear)
                        }
                    }
                } header: {
                    if discs.count > 1 {
                        Text(said("album.disc", Int(disc.number)))
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
        .background { CatalogReading(subject: key) { await load() } }
        .task(id: key) { await learnTheRelease() }
    }

    private func learnTheRelease() async {
        guard let native = try? await library.player() else { return }
        let key = key
        guard (try? await Work.aside { try native.learnAlbumSongs(key: key) }) == true else { return }
        guard !Task.isCancelled, self.key == key else { return }
        await load()
    }

    private func load() async {
        guard let native = try? await library.player() else { return }
        let key = key
        let found: AlbumHit?
        let rows: [Track]
        let lacking: [SearchSong]
        do {
            (found, rows, lacking) = try await Work.onScreen {
                (try native.album(key: key), try native.albumTracks(key: key), try native.albumSongsNotHeld(key: key))
            }
        } catch {
            failure = explained(error)
            return
        }
        guard !Task.isCancelled, self.key == key else { return }
        failure = nil
        if rows.isEmpty, let shown = tracks.compactMap({ library.artworkTracks[$0.id] }).first {
            let moved = albumKey(album: shown.album, artist: shown.artist, albumArtist: shown.albumArtist)
            guard let moved else { return dismiss() }
            guard moved == key else {
                self.key = moved
                return
            }
        }
        if album != found { album = found }
        if tracks != rows { tracks = rows }
        if notHeld != lacking { notHeld = lacking }
        if !read { read = true }
    }
}

struct NotHeldSongRow: View {
    @Environment(Library.self) private var library
    @ScaledMetric(relativeTo: .callout) private var numberWidth: CGFloat = 28
    let song: SearchSong
    var numbered = true
    var showsAlbum = false

    var body: some View {
        let features = library.features
        let downloads = features.downloads
        let standing = downloads.standing(of: song)
        Button {
            guard standing.startsOnTap else { return }
            downloads.enqueue([song])
        } label: {
            HStack(spacing: 12) {
                if numbered {
                    Text(song.position.map(String.init) ?? "")
                        .font(.callout.monospacedDigit())
                        .foregroundStyle(.secondary)
                        .frame(width: numberWidth)
                }
                VStack(alignment: .leading, spacing: 2) {
                    Text(song.title)
                        .font(.body)
                        .foregroundStyle(.secondary)
                        .titleLines()
                    if showsAlbum, !song.album.isEmpty {
                        Text(song.album)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                            .titleLines()
                    }
                    if standing != .available {
                        OnlineSongCaption(song: song, standing: standing)
                    }
                }
                Spacer(minLength: 0)
                if standing == .available, let duration = song.durationMs {
                    Text(clock(duration))
                        .font(.subheadline.monospacedDigit())
                        .foregroundStyle(.secondary)
                }
                DownloadBadge(standing: standing) { downloads.cancel(song) }
            }
            .contentShape(.rect)
        }
        .buttonStyle(.plain)
        .swipeActions(edge: .trailing) {
            Button("song.save-for-later", systemImage: "bookmark") { features.saveWanted([song]) }
                .tint(.indigo)
        }
        .contextMenu {
            Button("song.download", systemImage: "arrow.down.circle") { downloads.enqueue([song]) }
                .disabled(!standing.startsOnTap)
            Button("song.save-for-later", systemImage: "bookmark") { features.saveWanted([song]) }
        }
        .accessibilityElement(children: .combine)
        .accessibilityValue("song.not-in-library")
        .accessibilityHint(standing.startsOnTap ? said("song.download.hint") : "")
        .accessibilityActions {
            if standing.cancellable {
                Button("song.download.cancel") { downloads.cancel(song) }
            }
        }
    }
}

struct ArtistView: View {
    private static let topSongs: UInt32 = 5

    @Environment(Library.self) private var library
    @Environment(\.scenePhase) private var phase
    @ScaledMetric(relativeTo: .subheadline) private var albumEdge: CGFloat = 140
    @State private var name: String
    @State private var albums: [AlbumHit] = []
    @State private var tracks: [Track] = []
    @State private var top: [Track] = []
    @State private var releasesLacked: [ReleaseNotHeld] = []
    @State private var songsLacked: [SearchSong] = []
    @State private var openRelease: ReleaseNotHeld?
    @State private var seeingAll = false
    @State private var seeingAllLacked = false
    @State private var read = false
    @State private var failure: String?

    init(name: String) {
        _name = State(initialValue: name)
    }

    var body: some View {
        let shownName = name.isEmpty ? said("artist.unknown") : name
        List {
            CollectionHeader(
                title: shownName,
                subtitle: said("artist.subtitle"),
                tracks: tracks,
                summary: "\(said("counted.albums", albums.count)) · \(said("counted.songs", tracks.count))",
                circle: true
            )
            if let failure {
                ReadFailure(text: failure) { Task { await load() } }
            } else if read, tracks.isEmpty {
                Text("artist.empty").foregroundStyle(.secondary)
            }
            if !albums.isEmpty {
                Section("library.albums") {
                    ScrollView(.horizontal, showsIndicators: false) {
                        LazyHStack(spacing: 14) {
                            ForEach(albums, id: \.key) { album in
                                NavigationLink(value: album.destination) {
                                    VStack(alignment: .leading) {
                                        ArtworkView(track: library.artworkTrack(album.artworkTrack), corner: 10)
                                            .frame(width: albumEdge, height: albumEdge)
                                        Text(album.shownTitle)
                                            .font(.subheadline.weight(.medium))
                                            .titleLines()
                                        Text(album.year.map(String.init) ?? said("counted.songs", Int(album.songs)))
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
            if !releasesLacked.isEmpty {
                Section(NotInYourLibrary.heading(releasesLacked.count, .release)) {
                    ScrollView(.horizontal, showsIndicators: false) {
                        LazyHStack(spacing: 14) {
                            ForEach(releasesLacked, id: \.group) { release in
                                Button {
                                    openRelease = release
                                } label: {
                                    VStack(alignment: .leading) {
                                        UnheldSleeve(release: release, edge: albumEdge)
                                        Text(release.title)
                                            .font(.subheadline.weight(.medium))
                                            .foregroundStyle(.secondary)
                                            .titleLines()
                                        Text(NotInYourLibrary.line(release))
                                            .font(.caption)
                                            .foregroundStyle(.secondary)
                                    }
                                    .frame(width: albumEdge)
                                    .accessibilityElement(children: .combine)
                                    .accessibilityValue("song.not-in-library")
                                    .accessibilityHint("artist.lacked.release.hint")
                                }
                                .buttonStyle(.plain)
                            }
                        }
                    }
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
                    SeeAllHeader(title: seeingAll ? said("artist.songs") : said("artist.top-songs"), more: !seeingAll && tracks.count > top.count) {
                        seeingAll = true
                    }
                }
                .listRowBackground(Color.clear)
            }
            if !songsLacked.isEmpty {
                let lacked = NotInYourLibrary.shown(songsLacked, all: seeingAllLacked)
                Section {
                    ForEach(lacked, id: \.id) { song in
                        NotHeldSongRow(song: song, numbered: false, showsAlbum: true)
                            .listRowBackground(Color.clear)
                    }
                    if lacked.count < songsLacked.count {
                        Button(said("artist.lacked.show-all", songsLacked.count)) { seeingAllLacked = true }
                            .listRowBackground(Color.clear)
                    }
                } header: {
                    Text(NotInYourLibrary.heading(songsLacked.count, .song))
                }
            }
        }
        .listStyle(.plain)
        .background(BackdropView(track: tracks.first, moving: false))
        .scrollContentBackground(.hidden)
        .navigationTitle(shownName)
        .navigationBarTitleDisplayMode(.inline)
        .toolbar { CollectionMenu(tracks: tracks) }
        .sheet(item: $openRelease) { release in
            UnheldReleaseSheet(artist: name, release: release)
        }
        .background { CatalogReading(subject: name) { await load() } }
        .task(id: "\(name):\(phase == .background)") { await learnTheDiscography() }
    }

    private func learnTheDiscography() async {
        guard phase != .background, let native = try? await library.player() else { return }
        let name = name
        if (try? await Work.aside { try native.learnArtistDiscography(name: name) }) == true {
            await readWhatIsLacked(native)
        }
        while !Task.isCancelled {
            let read = (try? await Work.aside { try native.learnArtistSongs(name: name) }) ?? 0
            await readWhatIsLacked(native)
            if read == 0 { break }
        }
        while !Task.isCancelled {
            let covered = (try? await Work.aside { try native.learnArtistCovers(name: name) }) ?? 0
            await readWhatIsLacked(native)
            if covered == 0 { break }
        }
    }

    private func readWhatIsLacked(_ native: MobilePlayer) async {
        let name = name
        guard let (releases, songs) = try? await Work.onScreen({
            (try native.artistReleasesNotHeld(name: name), try native.artistSongsNotHeld(name: name))
        }) else { return }
        if releasesLacked != releases { releasesLacked = releases }
        if songsLacked != songs { songsLacked = songs }
    }

    private func load() async {
        guard let native = try? await library.player() else { return }
        let name = name
        let most = Self.topSongs
        let found: [AlbumHit]
        let rows: [Track]
        let played: [Track]
        let releases: [ReleaseNotHeld]
        let songs: [SearchSong]
        do {
            (found, rows, played, releases, songs) = try await Work.onScreen {
                (
                    try native.albumsBy(name: name),
                    try native.artistTracks(name: name),
                    try native.mostPlayedBy(name: name, limit: most),
                    try native.artistReleasesNotHeld(name: name),
                    try native.artistSongsNotHeld(name: name)
                )
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
        if albums != found { albums = found }
        if tracks != rows { tracks = rows }
        if top != played { top = played }
        if releasesLacked != releases { releasesLacked = releases }
        if songsLacked != songs { songsLacked = songs }
        if !read { read = true }
    }
}

extension ReleaseNotHeld: Identifiable {
    public var id: String { group }
}

final class UnheldSleeves: @unchecked Sendable {
    static let shared = UnheldSleeves()

    private static let costLimit = 16 * 1024 * 1024

    private let cache: NSCache<NSString, UIImage> = {
        let cache = NSCache<NSString, UIImage>()
        cache.totalCostLimit = costLimit
        return cache
    }()

    static func name(of group: String, edge: Int) -> String {
        "\(group)@\(edge)"
    }

    func picture(named name: String) -> UIImage? {
        cache.object(forKey: name as NSString)
    }

    func keep(_ picture: UIImage, named name: String) {
        cache.setObject(picture, forKey: name as NSString, cost: ArtworkDecoder.cost(of: picture))
    }
}

struct UnheldSleeve: View {
    @Environment(Library.self) private var library
    @Environment(\.displayScale) private var displayScale
    let release: ReleaseNotHeld
    let edge: CGFloat
    @State private var picture: UIImage?

    var body: some View {
        Group {
            if let picture {
                Image(uiImage: picture)
                    .resizable()
                    .scaledToFill()
                    .frame(width: edge, height: edge)
                    .clipped()
                    .modifier(ArtworkClip(circle: false, corner: 8))
                    .opacity(0.5)
                    .accessibilityHidden(true)
            } else {
                OnlinePlaceholder(symbol: "opticaldisc")
                    .frame(width: edge, height: edge)
            }
        }
        .task(id: "\(release.group):\(release.covered)") {
            guard release.covered, let native = try? await library.player() else {
                picture = nil
                return
            }
            let group = release.group
            let pixels = Int((edge * displayScale).rounded(.up))
            let name = UnheldSleeves.name(of: group, edge: pixels)
            if let kept = UnheldSleeves.shared.picture(named: name) {
                picture = kept
                return
            }
            let decoded = try? await Work.aside { () -> UIImage? in
                guard let bytes = try native.unheldReleaseCover(group: group), let image = ArtworkDecoder.thumbnail(bytes, edge: pixels) else {
                    return nil
                }
                UnheldSleeves.shared.keep(image, named: name)
                return image
            }
            guard !Task.isCancelled else { return }
            picture = decoded
        }
    }
}

struct UnheldReleaseSheet: View {
    @Environment(Library.self) private var library
    @Environment(\.dismiss) private var dismiss
    let artist: String
    let release: ReleaseNotHeld
    @State private var songs: [SearchSong] = []
    @State private var read = false
    @State private var failed = false

    var body: some View {
        NavigationStack {
            List {
                DownloadProgress()
                if !songs.isEmpty {
                    Section {
                        Button(said("release.download-all", songs.count), systemImage: "arrow.down.circle") {
                            library.features.downloads.enqueue(songs)
                        }
                    }
                    Section(NotInYourLibrary.line(release)) {
                        ForEach(songs, id: \.id) { song in
                            NotHeldSongRow(song: song)
                        }
                    }
                }
                if failed {
                    ReadFailure(text: said("release.unread")) { Task { await readSongs() } }
                } else if read, songs.isEmpty {
                    Text("release.nothing-missing").foregroundStyle(.secondary)
                }
            }
            .overlay { if !read, !failed { ProgressView().accessibilityLabel("common.loading") } }
            .navigationTitle(release.title)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .confirmationAction) { Button("common.done") { dismiss() } } }
            .task { await readSongs() }
        }
    }

    private func readSongs() async {
        guard let native = try? await library.player() else { return }
        let artist = artist
        let group = release.group
        failed = false
        do {
            if !release.songsRead {
                _ = try await Work.aside { try native.learnReleaseSongs(group: group) }
            }
            songs = try await Work.onScreen { try native.releaseSongsNotHeld(artist: artist, group: group) }
            read = true
        } catch {
            failed = true
        }
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
    @State private var keyedAt: Listed?
    @State private var found: [Track] = []
    @State private var foundFor = ""
    @State private var ordered: OrderedFavourites?

    private struct Asked: Equatable {
        let listed: Listed
        let folded: String
    }

    private struct Listed: Equatable {
        let epoch: Int
        let ordering: SongOrdering
    }

    private struct OrderedFavourites {
        let listed: Listed
        let tracks: [Track]
    }

    var body: some View {
        @Bindable var library = library
        let folded = search.trimmingCharacters(in: .whitespacesAndNewlines).searchFolded
        let favourites = library.favouriteTracks
        let listing = Listed(epoch: library.favouriteEpoch, ordering: library.favouritesOrder)
        let listed = listing.ordering == .newestFirst ? favourites : ordered?.tracks ?? favourites
        let shown = folded.isEmpty ? listed : found
        List {
            CollectionHeader(title: said("favourites.title"), subtitle: "", tracks: favourites) {
                FavouritesArtwork(corner: CollectionCover.corner)
            }
            if favourites.isEmpty {
                VStack(spacing: 12) {
                    Text("favourites.empty")
                        .multilineTextAlignment(.center)
                        .foregroundStyle(.secondary)
                    Button("favourites.browse-songs") { navigation.showSongs() }.buttonStyle(.glass)
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
        .searchable(text: $search, prompt: "favourites.search.prompt")
        .task(id: listing) { await order(listing) }
        .task(id: Asked(listed: listing, folded: folded)) {
            guard !folded.isEmpty else { return }
            try? await Task.sleep(for: .milliseconds(150))
            guard !Task.isCancelled else { return }
            if keyedAt != listing {
                let tracks = listed
                keyed = (try? await Work.onScreen { SongKeys.of(tracks) }) ?? keyed
                guard !Task.isCancelled else { return }
                keyedAt = listing
            }
            let keys = keyed
            let matching = (try? await Work.onScreen { keys.matching(folded) }) ?? []
            guard !Task.isCancelled else { return }
            found = matching
            foundFor = folded
        }
        .navigationTitle("favourites.title")
        .toolbar {
            if !favourites.isEmpty {
                ToolbarItem(placement: .topBarTrailing) { SongOrderMenu(ordering: $library.favouritesOrder) }
                ToolbarSpacer(.fixed, placement: .topBarTrailing)
            }
            ToolbarItem(placement: .topBarTrailing) { CollectionMenu(tracks: favourites) }
        }
        .navigationBarTitleDisplayMode(.inline)
    }

    private func order(_ listing: Listed) async {
        guard listing.ordering != .newestFirst else {
            ordered = nil
            return
        }
        guard ordered?.listed != listing else { return }
        let filter = listing.ordering.favourites()
        let favourites = library.favouriteTracks
        do {
            let native = try await library.player()
            let read = try await Work.onScreen {
                let held = Set(favourites.map(\.id))
                return try Library.everyPage { try native.filteredTracks(filter: filter, offset: $0, limit: Library.page) }
                    .filter { held.contains($0.id) }
            }
            guard !Task.isCancelled else { return }
            ordered = OrderedFavourites(listed: listing, tracks: read)
        } catch {
            guard !Task.isCancelled else { return }
            library.report(error)
        }
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

    func body(content: Content) -> some View {
        content
            .contextMenu {
                Section {
                    Button("common.play", systemImage: "play.fill") { act { library.play($0, at: 0) } }
                    Button("common.shuffle", systemImage: "shuffle") { act { library.shuffle($0) } }
                }
                Section {
                    Button("song.play-next", systemImage: "text.line.first.and.arrowtriangle.forward") { act { library.playNext($0) } }
                    Button("song.add-to-queue", systemImage: "text.line.last.and.arrowtriangle.forward") { act { library.addToQueue($0) } }
                }
                Section {
                    Button("song.add-to-playlist", systemImage: "text.badge.plus") { act { navigation.addToPlaylist($0) } }
                    if let artist, !artist.isEmpty {
                        Button("song.go-to-artist", systemImage: "music.mic") { navigation.open(.artist(artist)) }
                    }
                }
            }
            .swipeActions(edge: .leading, allowsFullSwipe: true) {
                Button("song.play-next", systemImage: "text.line.first.and.arrowtriangle.forward") { act { library.playNext($0) } }
                    .tint(.indigo)
                Button("song.add-to-queue", systemImage: "text.line.last.and.arrowtriangle.forward") { act { library.addToQueue($0) } }
                    .tint(.orange)
            }
    }

    private func act(_ action: @escaping @MainActor ([Track]) -> Void) {
        let source = source
        Task {
            do {
                let native = try await library.player()
                let tracks = try await Work.onScreen { try source.tracks(native) }
                guard !tracks.isEmpty else { return }
                navigation.feelQueued()
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

struct FavouritesCountRow: View {
    @Environment(Library.self) private var library

    var body: some View {
        FavouritesRow(count: library.favouriteTracks.count)
    }
}
