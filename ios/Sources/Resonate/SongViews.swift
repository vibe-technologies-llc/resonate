import ResonateNative
import SwiftUI

extension Track {
    var shownTitle: String { title.isEmpty ? name : title }
    var shownArtist: String { artist.isEmpty ? "Unknown artist" : artist }
    var groupingArtist: String { albumArtist.isEmpty ? artist : albumArtist }
    var albumPage: Destination? { albumKey(album: album, artist: artist, albumArtist: albumArtist).map(Destination.album) }

    var quality: String {
        var parts = [codec]
        if let bits { parts.append("\(bits)-bit") }
        if rate > 0 { parts.append(String(format: "%g kHz", Double(rate) / 1000)) }
        return parts.filter { !$0.isEmpty }.joined(separator: " · ")
    }
}

enum Grouping: String, CaseIterable, Identifiable {
    case songs = "Songs"
    case albums = "Albums"
    case artists = "Artists"

    var id: String { rawValue }
}

struct SongsView: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    @State private var scope = SearchScope.top

    var body: some View {
        @Bindable var library = library
        @Bindable var navigation = navigation
        let grouping = navigation.songsGrouping
        @Bindable var search = library.songSearch
        List {
            if search.isAsked {
                SearchResults(search: search, scope: $scope).themedRows()
            } else {
                Section {
                    Picker("Show", selection: $navigation.songsGrouping) {
                        ForEach(Grouping.allCases) { Text($0.rawValue).tag($0) }
                    }
                    .pickerStyle(.segmented)
                    .labelsHidden()
                    .listRowInsets(EdgeInsets(top: 8, leading: 16, bottom: 8, trailing: 16))
                    if grouping == .songs {
                        FilterChips()
                    } else {
                        CollectionOrderPicker(order: $library.collectionOrder, grouping: grouping)
                    }
                }
                .listRowSeparator(.hidden)
                .themedRows()
                BrowsedMusic(grouping: grouping).themedRows()
            }
        }
        .listStyle(.plain)
        .listSectionIndexVisibility(.visible)
        .searchable(text: $search.text, prompt: "Search your music")
        .searchScopes($scope, activation: .onSearchPresentation) {
            ForEach(SearchScope.allCases) { Text($0.rawValue).tag($0) }
        }
        .searchSuggestions { SearchSuggestions(search: search) }
        .onSubmit(of: .search) { search.remember() }
        .onAppear { search.recallRecents() }
        .navigationTitle("Resonate")
        .toolbar {
            if grouping == .songs, !search.isAsked {
                ToolbarItem(placement: .topBarTrailing) { FilterButton() }
            }
            LibraryToolbar()
            if search.asking, search.hasAnswer {
                ToolbarItem(placement: .topBarLeading) { ProgressView() }
            }
        }
    }
}

struct BrowsedMusic: View {
    @Environment(Library.self) private var library
    let grouping: Grouping

    var body: some View {
        if library.catalogEmpty {
            EmptyLibrary()
        } else {
            switch grouping {
            case .songs: BrowsedSongs()
            case .albums: BrowsedAlbums()
            case .artists: BrowsedArtists()
            }
        }
    }
}

struct BrowsedSongs: View {
    @Environment(Library.self) private var library

    var body: some View {
        let tracks = library.tracks
        if library.tracksRead, tracks.isEmpty, library.filtering {
            ContentUnavailableView {
                Label("No songs match these filters", systemImage: "line.3.horizontal.decrease.circle")
            } description: {
                Text("Change or reset the filters to see the rest of your library.")
            } actions: {
                Button("Reset filters") { library.setFilter(defaultTrackFilter()) }
                    .buttonStyle(.glass)
            }
            .listRowSeparator(.hidden)
        } else if library.trackRuns.isEmpty {
            ForEach(tracks, id: \.id) { row($0, among: tracks) }
        } else {
            ForEach(library.trackRuns) { run in
                Section {
                    ForEach(tracks[tracks.clamped(run.rows)], id: \.id) { row($0, among: tracks) }
                } header: {
                    Text(run.letter)
                }
                .sectionIndexLabel(run.indexed ? run.letter : nil)
            }
        }
    }

    private func row(_ track: Track, among tracks: [Track]) -> some View {
        TrackRow(track: track, current: library.snapshot?.current?.id == track.id) {
            library.play(tracks, selected: track)
        }
    }
}

struct BrowsedAlbums: View {
    @Environment(Library.self) private var library

    var body: some View {
        let albums = library.albums
        if library.albumRuns.isEmpty {
            ForEach(albums, id: \.key) { link($0) }
        } else {
            ForEach(library.albumRuns) { run in
                Section {
                    ForEach(albums[albums.clamped(run.rows)], id: \.key) { link($0) }
                } header: {
                    Text(run.letter)
                }
                .sectionIndexLabel(run.indexed ? run.letter : nil)
            }
        }
    }

    private func link(_ album: AlbumHit) -> some View {
        NavigationLink(value: album.destination) {
            CollectionRow(track: library.artworkTrack(album.artworkTrack), title: album.shownTitle, detail: album.detail)
        }
        .collectionActions(.album(album.key), artist: album.artist)
    }
}

struct BrowsedArtists: View {
    @Environment(Library.self) private var library

    var body: some View {
        let artists = library.artists
        if library.artistRuns.isEmpty {
            ForEach(artists, id: \.name) { link($0) }
        } else {
            ForEach(library.artistRuns) { run in
                Section {
                    ForEach(artists[artists.clamped(run.rows)], id: \.name) { link($0) }
                } header: {
                    Text(run.letter)
                }
                .sectionIndexLabel(run.indexed ? run.letter : nil)
            }
        }
    }

    private func link(_ artist: ArtistHit) -> some View {
        NavigationLink(value: artist.destination) {
            CollectionRow(track: library.artworkTrack(artist.artworkTrack), title: artist.shownName, detail: artist.detail, circle: true)
        }
        .collectionActions(.artist(artist.name))
    }
}

struct CollectionOrderPicker: View {
    @Binding var order: CollectionOrder
    let grouping: Grouping

    var body: some View {
        Picker("Sort by", selection: $order) {
            Text("Name").tag(CollectionOrder.alphabetical)
            if grouping == .albums {
                Text("Artist").tag(CollectionOrder.artist)
                Text("Year").tag(CollectionOrder.year)
            }
            Text("Recently added").tag(CollectionOrder.added)
            Text("Most played").tag(CollectionOrder.plays)
        }
        .pickerStyle(.menu)
    }
}

struct FilterChips: View {
    @Environment(Library.self) private var library

    var body: some View {
        let filter = library.filter
        let chips = FilterChip.of(filter)
        if !chips.isEmpty {
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 6) {
                    ForEach(chips) { chip in
                        Button { library.setFilter(chip.cleared(from: filter)) } label: {
                            FilterToken(label: chip.label)
                        }
                        .buttonStyle(.borderless)
                        .touchTarget()
                        .accessibilityLabel("Remove filter \(chip.label)")
                    }
                }
                .padding(.horizontal, 16)
            }
            .scrollClipDisabled()
            .listRowInsets(EdgeInsets(top: 0, leading: 0, bottom: 6, trailing: 0))
        }
    }
}

struct FilterToken: View {
    let label: String

    var body: some View {
        HStack(spacing: 4) {
            Text(label).lineLimit(1)
            Image(systemName: "xmark")
                .font(.caption2.weight(.bold))
                .foregroundStyle(.secondary)
        }
        .font(.footnote.weight(.medium))
        .foregroundStyle(.primary)
        .padding(.horizontal, 10)
        .padding(.vertical, 5)
        .background(.quaternary, in: .capsule)
        .contentShape(.capsule)
    }
}

struct FilterButton: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation

    var body: some View {
        let filtering = library.filtering
        Button("Filter and sort", systemImage: filtering ? "line.3.horizontal.decrease.circle.fill" : "line.3.horizontal.decrease.circle") {
            navigation.filters = true
        }
        .accessibilityValue(filtering ? "On" : "Off")
    }
}

enum FilterChip: Identifiable {
    case favourites
    case unplayed
    case hiRes
    case depth(UInt32)
    case genre(String)
    case artist(String)
    case album(String)
    case order(TrackOrder, descending: Bool)

    static let orderNames: [(TrackOrder, String)] = [
        (.title, "Title"), (.album, "Album"), (.artist, "Artist"), (.duration, "Duration"), (.added, "Added"), (.plays, "Plays"), (.played, "Last played"),
    ]

    static func of(_ filter: TrackFilter) -> [FilterChip] {
        var chips: [FilterChip] = []
        if filter.favourites { chips.append(.favourites) }
        if filter.unplayed { chips.append(.unplayed) }
        if filter.minimumRate > 0 { chips.append(.hiRes) }
        if filter.minimumBits > 0 { chips.append(.depth(filter.minimumBits)) }
        if !filter.genre.isEmpty { chips.append(.genre(filter.genre)) }
        if !filter.artist.isEmpty { chips.append(.artist(filter.artist)) }
        if !filter.album.isEmpty { chips.append(.album(filter.album)) }
        if filter.order != .title || filter.descending { chips.append(.order(filter.order, descending: filter.descending)) }
        return chips
    }

    var id: String { label }

    var label: String {
        switch self {
        case .favourites: "Favourites"
        case .unplayed: "Unplayed"
        case .hiRes: "Hi-res"
        case .depth(let bits): "\(bits)-bit or more"
        case .genre(let genre): "Genre: \(genre)"
        case .artist(let artist): "Artist: \(artist)"
        case .album(let album): "Album: \(album)"
        case .order(let order, let descending):
            "Sorted by \(Self.orderNames.first { $0.0 == order }?.1.lowercased() ?? "title")\(descending ? ", descending" : "")"
        }
    }

    func cleared(from filter: TrackFilter) -> TrackFilter {
        var cleared = filter
        switch self {
        case .favourites: cleared.favourites = false
        case .unplayed: cleared.unplayed = false
        case .hiRes: cleared.minimumRate = 0
        case .depth: cleared.minimumBits = 0
        case .genre: cleared.genre = ""
        case .artist: cleared.artist = ""
        case .album: cleared.album = ""
        case .order:
            cleared.order = .title
            cleared.descending = false
        }
        return cleared
    }
}

struct EmptyLibrary: View {
    @Environment(Navigation.self) private var navigation

    var body: some View {
        VStack(spacing: 16) {
            Image(systemName: "music.note.house")
                .font(.system(size: 56))
                .foregroundStyle(.tint)
            Text("Your music, in its own detail").font(.title2.weight(.semibold))
            Text("Choose a music folder to build your library, or put songs in Resonate's folder in the Files app. Your files stay where they are.")
                .multilineTextAlignment(.center)
                .foregroundStyle(.secondary)
            Button("Choose music folder") { navigation.choosingFolder = true }
                .buttonStyle(.glassProminent)
        }
        .frame(maxWidth: .infinity)
        .padding(32)
        .listRowSeparator(.hidden)
    }
}

struct CollectionRow: View {
    @ScaledMetric(relativeTo: .body) private var artworkEdge: CGFloat = 56
    let track: Track?
    let title: AttributedString
    let detail: String
    let circle: Bool
    let disclosed: Bool

    init(track: Track?, title: AttributedString, detail: String, circle: Bool = false, disclosed: Bool = false) {
        self.track = track
        self.title = title
        self.detail = detail
        self.circle = circle
        self.disclosed = disclosed
    }

    init(track: Track?, title: String, detail: String, circle: Bool = false, disclosed: Bool = false) {
        self.init(track: track, title: AttributedString(title), detail: detail, circle: circle, disclosed: disclosed)
    }

    var body: some View {
        HStack(spacing: 14) {
            ArtworkView(track: track, corner: 10, circle: circle)
                .frame(width: artworkEdge, height: artworkEdge)
            VStack(alignment: .leading, spacing: 2) {
                Text(title).font(.body.weight(.medium)).lineLimit(1)
                Text(detail).font(.subheadline).foregroundStyle(.secondary).lineLimit(1)
            }
            Spacer(minLength: 0)
            if disclosed {
                Image(systemName: "chevron.right")
                    .font(.footnote.weight(.semibold))
                    .foregroundStyle(.tertiary)
            }
        }
        .contentShape(.rect)
        .accessibilityElement(children: .combine)
    }
}

struct TrackRow: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    @ScaledMetric(relativeTo: .body) private var artworkEdge: CGFloat = 48
    @ScaledMetric(relativeTo: .callout) private var numberWidth: CGFloat = 28
    let track: Track
    var current: Bool?
    var numbered: UInt32?
    var albumArtist: String?
    var showsAlbum = false
    var query: String?
    var ownsMenus = true
    let play: () -> Void
    @State private var choosingPlaylist = false
    @State private var queued = 0

    var body: some View {
        let playingHere = current ?? (library.currentId == track.id)
        HStack(spacing: 12) {
            Button(action: play) {
                HStack(spacing: 12) {
                    leading(playingHere)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(query.map { lit(track.shownTitle, by: $0, in: .title) } ?? AttributedString(track.shownTitle))
                            .font(.body.weight(playingHere ? .semibold : .regular))
                            .foregroundStyle(playingHere ? Color.worn : Color.primary)
                            .lineLimit(1)
                        if let detail {
                            Text(detail)
                                .font(.subheadline)
                                .foregroundStyle(.secondary)
                                .lineLimit(1)
                        }
                    }
                    Spacer(minLength: 0)
                    if albumArtist != nil {
                        Text(clock(track.durationMs))
                            .font(.subheadline.monospacedDigit())
                            .foregroundStyle(.secondary)
                    }
                }
                .contentShape(.rect)
            }
            .buttonStyle(.plain)
            .accessibilityValue(playingHere ? "Now playing" : "")
            FavouriteButton(track: track)
        }
        .swipeActions(edge: .leading, allowsFullSwipe: true) {
            Button("Play next", systemImage: "text.line.first.and.arrowtriangle.forward") {
                queued += 1
                library.playNext([track])
            }
            .tint(.indigo)
            Button("Add to queue", systemImage: "text.line.last.and.arrowtriangle.forward") {
                queued += 1
                library.addToQueue([track])
            }
            .tint(.orange)
        }
        .modifier(StandardTrackMenus(track: track, enabled: ownsMenus, choosingPlaylist: $choosingPlaylist, queued: $queued))
        .sensoryFeedback(.success, trigger: queued)
        .sheet(isPresented: $choosingPlaylist) { PlaylistPicker(tracks: [track]) }
        .accessibilityAction(named: "Song details") { navigation.open(.inspector(track)) }
    }

    private var detail: AttributedString? {
        if let query {
            var shown = lit(track.shownArtist, by: query, in: .artist)
            if !track.album.isEmpty {
                shown.append(AttributedString(" · "))
                shown.append(lit(track.album, by: query, in: .album))
            }
            return shown
        }
        if showsAlbum {
            return AttributedString(track.album.isEmpty ? clock(track.durationMs) : "\(track.album) · \(clock(track.durationMs))")
        }
        guard let albumArtist else { return AttributedString("\(track.shownArtist) · \(clock(track.durationMs))") }
        let credited = track.artist.trimmingCharacters(in: .whitespaces)
        guard !credited.isEmpty, credited.searchFolded != albumArtist.trimmingCharacters(in: .whitespaces).searchFolded else { return nil }
        return AttributedString(credited)
    }

    @ViewBuilder
    private func leading(_ playingHere: Bool) -> some View {
        if let numbered {
            ZStack {
                if playingHere {
                    NowPlayingIndicator()
                } else {
                    Text("\(numbered)").font(.callout.monospacedDigit()).foregroundStyle(.secondary)
                }
            }
            .frame(width: numberWidth)
        } else {
            ArtworkView(track: track, corner: 8)
                .frame(width: artworkEdge, height: artworkEdge)
                .overlay {
                    if playingHere {
                        NowPlayingIndicator()
                            .foregroundStyle(.white)
                            .frame(maxWidth: .infinity, maxHeight: .infinity)
                            .background(.black.opacity(0.35), in: .rect(cornerRadius: 8))
                    }
                }
        }
    }
}

struct StandardTrackMenus: ViewModifier {
    @Environment(Library.self) private var library
    let track: Track
    let enabled: Bool
    @Binding var choosingPlaylist: Bool
    @Binding var queued: Int

    func body(content: Content) -> some View {
        if enabled {
            let favourite = library.isFavourite(track)
            content
                .swipeActions(edge: .trailing) {
                    Button(favourite ? "Remove from favourites" : "Add to favourites", systemImage: favourite ? "heart.slash" : "heart") {
                        library.setFavourite(track, !favourite)
                    }
                    .tint(.pink)
                    Button("Add to playlist", systemImage: "text.badge.plus") { choosingPlaylist = true }
                        .tint(.blue)
                }
                .contextMenu { TrackMenu(track: track, choosingPlaylist: $choosingPlaylist, queued: $queued) }
        } else {
            content
        }
    }
}

struct NowPlayingIndicator: View {
    @Environment(Library.self) private var library
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        Image(systemName: "waveform")
            .font(.callout.weight(.semibold))
            .foregroundStyle(.tint)
            .symbolEffect(.variableColor.iterative.dimInactiveLayers, isActive: library.transport == .playing && !reduceMotion)
            .accessibilityHidden(true)
    }
}

struct TrackMenu: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    let track: Track
    @Binding var choosingPlaylist: Bool
    var queued: Binding<Int>?

    var body: some View {
        let favourite = library.isFavourite(track)
        Section {
            Button("Play next", systemImage: "text.line.first.and.arrowtriangle.forward") {
                queued?.wrappedValue += 1
                library.playNext([track])
            }
            Button("Add to queue", systemImage: "text.line.last.and.arrowtriangle.forward") {
                queued?.wrappedValue += 1
                library.addToQueue([track])
            }
        }
        Section {
            Button(favourite ? "Remove from favourites" : "Add to favourites", systemImage: favourite ? "heart.slash" : "heart") {
                library.setFavourite(track, !favourite)
            }
            Button("Add to playlist", systemImage: "text.badge.plus") { choosingPlaylist = true }
        }
        Section {
            Button("Song details", systemImage: "info.circle") { navigation.open(.inspector(track)) }
            Button("Share link", systemImage: "square.and.arrow.up") { library.features.share(track) }
            if library.features.tagsWritable(track) {
                Button("Edit tags", systemImage: "pencil") { navigation.tagging = TagEditing(tracks: [track]) }
            }
            Button("Lyrics", systemImage: "quote.bubble") { navigation.open(.lyrics(track)) }
            Button("Analyse", systemImage: "waveform.path.ecg") { navigation.open(.analysis(track)) }
            if let albumPage = track.albumPage {
                Button("Go to album", systemImage: "square.stack") { navigation.open(albumPage) }
            }
            if !track.artist.isEmpty {
                Button("Go to artist", systemImage: "music.mic") { navigation.open(.artist(track.artist)) }
            }
        }
    }
}
