import ResonateLogic
import ResonateNative
import SwiftUI

extension Track {
    var albumPage: Destination? { isServed ? nil : albumKey(album: album, artist: artist, albumArtist: albumArtist).map(Destination.album) }
}

enum Grouping: String, CaseIterable, Identifiable {
    case songs
    case albums
    case artists

    var id: String { rawValue }

    var label: String {
        switch self {
        case .songs: said("library.songs")
        case .albums: said("library.albums")
        case .artists: said("library.artists")
        }
    }
}

struct SongsView: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    @State private var scope = SearchScope.top

    var body: some View {
        let grouping = navigation.songsGrouping
        @Bindable var search = library.songSearch
        Group {
            if search.isAsked {
                List {
                    SearchCompletions(search: search).themedRows()
                    SearchResults(search: search, scope: $scope) { Keyboard.stow() }.equatable().themedRows()
                }
                .listStyle(.plain)
                .scrollDismissesKeyboard(.immediately)
                .id(SongsList.searched)
            } else {
                browsed(grouping: grouping)
                    .id(SongsList.browsed)
            }
        }
        .searchable(text: $search.text, prompt: "songs.search.prompt")
        .searchScopes($scope, activation: .onSearchPresentation) {
            ForEach(SearchScope.allCases) { Text($0.label).tag($0) }
        }
        .searchSuggestions { SearchSuggestions(search: search) }
        .onSubmit(of: .search) { search.remember() }
        .onAppear { search.recallRecents() }
        .navigationTitle("library.songs")
        .toolbar {
            if grouping == .songs, !search.isAsked {
                ToolbarItem(placement: .topBarTrailing) { FilterButton() }
            }
            LibraryToolbar()
        }
    }

    private func browsed(grouping: Grouping) -> some View {
        @Bindable var library = library
        @Bindable var navigation = navigation
        return List {
            Section {
                Picker("songs.show", selection: $navigation.songsGrouping) {
                    ForEach(Grouping.allCases) { Text($0.label).tag($0) }
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
        .listStyle(.plain)
        .listSectionIndexVisibility(grouping == .songs ? .hidden : .visible)
    }
}

enum SongsList {
    case searched
    case browsed
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
                Label("songs.filtered.none", systemImage: "line.3.horizontal.decrease.circle")
            } description: {
                Text("songs.filtered.none.detail")
            } actions: {
                Button("songs.filtered.reset") { library.setFilter(Library.everything) }
                    .buttonStyle(.glass)
            }
            .listRowSeparator(.hidden)
        } else {
            ForEach(tracks, id: \.id) { row($0, among: tracks) }
        }
    }

    private func row(_ track: Track, among tracks: [Track]) -> some View {
        TrackRow(track: track) {
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
        Picker("filter.sort-by", selection: $order) {
            Text("collections.order.name").tag(CollectionOrder.alphabetical)
            if grouping == .albums {
                Text("collections.order.artist").tag(CollectionOrder.artist)
                Text("collections.order.year").tag(CollectionOrder.year)
            }
            Text("collections.order.added").tag(CollectionOrder.added)
            Text("collections.order.plays").tag(CollectionOrder.plays)
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
                        .accessibilityLabel(said("filter.chip.remove", chip.label))
                        .accessibilityInputLabels([chip.label, said("filter.chip.remove.spoken", chip.label)])
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
            Text(label).titleLines()
            Image(systemName: "xmark")
                .font(.caption2.weight(.bold))
                .foregroundStyle(.secondary)
                .accessibilityHidden(true)
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
        Button("filter.title", systemImage: filtering ? "line.3.horizontal.decrease.circle.fill" : "line.3.horizontal.decrease.circle") {
            navigation.filters = true
        }
        .accessibilityValue(said(filtering ? "common.on" : "common.off"))
    }
}

struct EmptyLibrary: View {
    @ScaledMetric(relativeTo: .largeTitle) private var glyph: CGFloat = 56
    @Environment(Navigation.self) private var navigation

    var body: some View {
        VStack(spacing: 16) {
            Image(systemName: "music.note.house")
                .font(.system(size: glyph))
                .foregroundStyle(.tint)
                .accessibilityHidden(true)
            Text("songs.empty.title").font(.title2.weight(.semibold))
            Text("songs.empty.detail")
                .multilineTextAlignment(.center)
                .foregroundStyle(.secondary)
            Button("songs.empty.choose-folder") { navigation.choosingFolder = true }
                .buttonStyle(.glassProminent)
        }
        .frame(maxWidth: .infinity)
        .padding(32)
        .listRowSeparator(.hidden)
    }
}

enum CollectionRowMeasure {
    case collection
    case song
}

struct CollectionRow: View {
    @ScaledMetric(relativeTo: .body) private var collectionEdge: CGFloat = 56
    @ScaledMetric(relativeTo: .body) private var songEdge: CGFloat = 48
    let track: Track?
    let title: AttributedString
    let detail: String
    let circle: Bool
    let disclosed: Bool
    let measure: CollectionRowMeasure

    init(
        track: Track?,
        title: AttributedString,
        detail: String,
        circle: Bool = false,
        disclosed: Bool = false,
        measure: CollectionRowMeasure = .collection
    ) {
        self.track = track
        self.title = title
        self.detail = detail
        self.circle = circle
        self.disclosed = disclosed
        self.measure = measure
    }

    init(
        track: Track?,
        title: String,
        detail: String,
        circle: Bool = false,
        disclosed: Bool = false,
        measure: CollectionRowMeasure = .collection
    ) {
        self.init(track: track, title: AttributedString(title), detail: detail, circle: circle, disclosed: disclosed, measure: measure)
    }

    var body: some View {
        let asSong = measure == .song
        let artworkEdge = asSong ? songEdge : collectionEdge
        HStack(spacing: asSong ? 12 : 14) {
            ArtworkView(track: track, corner: asSong ? 8 : 10, circle: circle)
                .frame(width: artworkEdge, height: artworkEdge)
            VStack(alignment: .leading, spacing: 2) {
                Text(title).font(.body.weight(.medium)).titleLines()
                Text(detail).font(.subheadline).foregroundStyle(.secondary).titleLines()
            }
            Spacer(minLength: 0)
            if disclosed {
                Image(systemName: "chevron.right")
                    .font(.footnote.weight(.semibold))
                    .foregroundStyle(.secondary)
                    .accessibilityHidden(true)
            }
        }
        .contentShape(.rect)
        .accessibilityElement(children: .combine)
    }
}

struct AlbumCredit {
    let name: String
    let folded: String

    init(_ artist: String) {
        name = artist.trimmingCharacters(in: .whitespaces)
        folded = name.searchFolded
    }

    func names(_ credited: String) -> Bool {
        credited == name || credited.searchFolded == folded
    }
}

struct TrackRow: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    let track: Track
    var current: Bool?
    var numbered: UInt32?
    var albumArtist: AlbumCredit?
    var showsAlbum = false
    var query: String?
    var ownsMenus = true
    let play: () -> Void

    var body: some View {
        HStack(spacing: 12) {
            PlayableTrack(
                track: track,
                title: title,
                detail: detail,
                current: current,
                numbered: numbered,
                timed: albumArtist != nil,
                play: play
            )
            FavouriteButton(track: track)
        }
        .swipeActions(edge: .leading, allowsFullSwipe: true) {
            Button("song.play-next", systemImage: "text.line.first.and.arrowtriangle.forward") {
                navigation.feelQueued()
                library.playNext([track])
            }
            .tint(.indigo)
            Button("song.add-to-queue", systemImage: "text.line.last.and.arrowtriangle.forward") {
                navigation.feelQueued()
                library.addToQueue([track])
            }
            .tint(.orange)
        }
        .modifier(StandardTrackMenus(track: track, enabled: ownsMenus))
        .accessibilityAction(named: "song.details") { navigation.open(.inspector(track)) }
    }

    private var title: AttributedString {
        query.map { lit(track.shownTitle, by: $0, in: .title) } ?? AttributedString(track.shownTitle)
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
        guard !credited.isEmpty, !albumArtist.names(credited) else { return nil }
        return AttributedString(credited)
    }
}

struct PlayableTrack: View {
    @Environment(Library.self) private var library
    @ScaledMetric(relativeTo: .body) private var artworkEdge: CGFloat = 48
    @ScaledMetric(relativeTo: .callout) private var numberWidth: CGFloat = 28
    let track: Track
    let title: AttributedString
    let detail: AttributedString?
    let current: Bool?
    let numbered: UInt32?
    let timed: Bool
    let play: () -> Void

    var body: some View {
        let playingHere = current ?? (library.currentId == track.id)
        Button(action: play) {
            HStack(spacing: 12) {
                leading(playingHere)
                VStack(alignment: .leading, spacing: 2) {
                    Text(title)
                        .font(.body.weight(playingHere ? .semibold : .regular))
                        .foregroundStyle(playingHere ? Color.worn : Color.primary)
                        .titleLines()
                    if let detail {
                        Text(detail)
                            .font(.subheadline)
                            .secondaryInk()
                            .titleLines()
                    }
                }
                Spacer(minLength: 0)
                if timed {
                    Text(clock(track.durationMs))
                        .font(.subheadline.monospacedDigit())
                        .foregroundStyle(.secondary)
                }
            }
            .contentShape(.rect)
        }
        .buttonStyle(.plain)
        .accessibilityValue(playingHere ? said("song.now-playing") : "")
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
    @Environment(Navigation.self) private var navigation
    let track: Track
    let enabled: Bool

    func body(content: Content) -> some View {
        if enabled {
            let favourite = library.isFavourite(track)
            content
                .swipeActions(edge: .trailing) {
                    Button(said(favourite ? "song.favourite.remove" : "song.favourite.add"), systemImage: favourite ? "heart.slash" : "heart") {
                        library.setFavourite(track, !favourite)
                    }
                    .tint(.pink)
                    Button("song.add-to-playlist", systemImage: "text.badge.plus") { navigation.addToPlaylist([track]) }
                        .tint(.blue)
                }
                .contextMenu { TrackMenu(track: track) }
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

    var body: some View {
        let favourite = library.isFavourite(track)
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
            Button(said(favourite ? "song.favourite.remove" : "song.favourite.add"), systemImage: favourite ? "heart.slash" : "heart") {
                library.setFavourite(track, !favourite)
            }
            Button("song.add-to-playlist", systemImage: "text.badge.plus") { navigation.addToPlaylist([track]) }
        }
        Section {
            SongPages(track: track)
        }
    }
}

struct SongPages: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    let track: Track

    var body: some View {
        if !track.isServed {
            pages
        }
    }

    @ViewBuilder
    private var pages: some View {
        Button("song.details", systemImage: "info.circle") { navigation.open(.inspector(track)) }
        ShareSongButton(track: track)
        if library.features.tagsWritable(track) {
            Button("song.edit-tags", systemImage: "pencil") { navigation.editTags([track]) }
        }
        Button("tools.lyrics", systemImage: "quote.bubble") { navigation.open(.lyrics(track)) }
        Button("song.analyse", systemImage: "waveform.path.ecg") { navigation.open(.analysis(track)) }
        if let albumPage = track.albumPage {
            Button("song.go-to-album", systemImage: "square.stack") { navigation.open(albumPage) }
        }
        if !track.artist.isEmpty {
            Button("song.go-to-artist", systemImage: "music.mic") { navigation.open(.artist(track.artist)) }
        }
        if library.features.songDeletable(track) {
            Button("song.delete", systemImage: "trash", role: .destructive) {
                navigation.deleting = SongsToDelete(tracks: [track])
            }
        }
    }
}

struct PlaylistPickerHost: ViewModifier {
    @Environment(Navigation.self) private var navigation
    let insidePlayer: Bool

    func body(content: Content) -> some View {
        content
            .sheet(item: Binding(
                get: { navigation.nowPlaying == insidePlayer ? navigation.addingToPlaylist : nil },
                set: { if $0 == nil { navigation.addingToPlaylist = nil } }
            )) { PlaylistPicker(tracks: $0.tracks).sheetSized() }
            .background { FeltFeedback() }
    }
}

private struct FeltFeedback: View {
    @Environment(Navigation.self) private var navigation

    var body: some View {
        Color.clear
            .sensoryFeedback(.success, trigger: navigation.queuedFelt)
            .sensoryFeedback(.selection, trigger: navigation.heartFelt)
            .accessibilityHidden(true)
    }
}
