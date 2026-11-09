import ResonateLogic
import ResonateNative
import SwiftUI

extension AlbumHit {
    var destination: Destination { .album(key) }
}

extension ArtistHit {
    var destination: Destination { .artist(name) }
}

struct SearchResults: View, Equatable {
    private static let topCollections = 6

    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    let search: SearchModel
    @Binding var scope: SearchScope
    let chosen: () -> Void
    let songsAtTop: Int
    let showsCollections: Bool
    private let searched: ObjectIdentifier
    private let scopeShown: SearchScope

    init(
        search: SearchModel,
        scope: Binding<SearchScope>,
        chosen: @escaping () -> Void = {},
        songsAtTop: Int = 5,
        showsCollections: Bool = true
    ) {
        self.search = search
        _scope = scope
        self.chosen = chosen
        self.songsAtTop = songsAtTop
        self.showsCollections = showsCollections
        searched = ObjectIdentifier(search)
        scopeShown = scope.wrappedValue
    }

    nonisolated static func == (shown: SearchResults, other: SearchResults) -> Bool {
        shown.searched == other.searched
            && shown.scopeShown == other.scopeShown
            && shown.songsAtTop == other.songsAtTop
            && shown.showsCollections == other.showsCollections
    }

    var body: some View {
        let found = search.found
        let query = search.answered

        if !found.reads.isEmpty {
            SearchReadings(reads: found.reads)
        }
        if search.matchedNothing {
            NothingFound(search: search)
        } else if search.hasAnswer {
            switch scope {
            case .top: top(found, query)
            case .songs: songs(found, query)
            case .albums: albums(found, query)
            case .artists: artists(found, query)
            }
        }
    }

    @ViewBuilder
    private func top(_ found: LibrarySearch, _ query: String) -> some View {
        Section {
            ForEach(found.tracks.prefix(songsAtTop), id: \.id) { track in
                songRow(track, among: found.tracks, query)
            }
        } header: {
            SeeAllHeader(title: SearchScope.songs.label, more: found.trackCount > songsAtTop) { scope = .songs }
        }
        if showsCollections, !found.albums.isEmpty {
            Section {
                AlbumShelf(albums: Array(found.albums.prefix(Self.topCollections)), query: query, picked: picked)
            } header: {
                SeeAllHeader(title: SearchScope.albums.label, more: found.albums.count > Self.topCollections) { scope = .albums }
            }
        }
        if showsCollections, !found.artists.isEmpty {
            Section {
                ArtistShelf(artists: Array(found.artists.prefix(Self.topCollections)), query: query, picked: picked)
            } header: {
                SeeAllHeader(title: SearchScope.artists.label, more: found.artists.count > Self.topCollections) { scope = .artists }
            }
        }
    }

    @ViewBuilder
    private func songs(_ found: LibrarySearch, _ query: String) -> some View {
        Section {
            ForEach(found.tracks, id: \.id) { track in
                songRow(track, among: found.tracks, query)
            }
        } header: {
            Text(said("counted.songs", Int(found.trackCount)))
        } footer: {
            if found.trackCount > found.tracks.count {
                Text(said("search.showing-best", found.tracks.count))
            }
        }
    }

    @ViewBuilder
    private func albums(_ found: LibrarySearch, _ query: String) -> some View {
        if found.albums.isEmpty {
            NoneMatch(kind: "albums", query: query, symbol: "square.stack")
        }
        ForEach(found.albums, id: \.key) { album in
            Button { open(album.destination) } label: {
                CollectionRow(
                    track: library.artworkTrack(album.artworkTrack),
                    title: lit(album.shownTitle, by: query, in: .album),
                    detail: album.detail,
                    disclosed: true
                )
            }
            .buttonStyle(.plain)
            .collectionActions(.album(album.key), artist: album.artist)
        }
    }

    @ViewBuilder
    private func artists(_ found: LibrarySearch, _ query: String) -> some View {
        if found.artists.isEmpty {
            NoneMatch(kind: "artists", query: query, symbol: "music.mic")
        }
        ForEach(found.artists, id: \.name) { artist in
            Button { open(artist.destination) } label: {
                CollectionRow(
                    track: library.artworkTrack(artist.artworkTrack),
                    title: lit(artist.shownName, by: query, in: .artist),
                    detail: artist.detail,
                    circle: true,
                    disclosed: true
                )
            }
            .buttonStyle(.plain)
            .collectionActions(.artist(artist.name))
        }
    }

    private func songRow(_ track: Track, among tracks: [Track], _ query: String) -> some View {
        TrackRow(track: track, query: query) {
            chosen()
            search.remember()
            if library.shuffled {
                let everything = library.allTracks
                if let place = everything.firstIndex(where: { $0.id == track.id }) {
                    library.play(everything, at: place, shuffle: true)
                } else {
                    library.play(tracks, at: tracks.firstIndex { $0.id == track.id } ?? 0, shuffle: true)
                }
            } else {
                library.play(tracks, selected: track)
            }
        }
    }

    private func open(_ destination: Destination) {
        picked()
        navigation.open(destination)
    }

    private func picked() {
        chosen()
        search.remember()
    }
}

@MainActor
enum Keyboard {
    static func stow() {
        UIApplication.shared.sendAction(#selector(UIResponder.resignFirstResponder), to: nil, from: nil, for: nil)
    }
}

struct SeeAllHeader: View {
    private static let reachBeyondTheLine: CGFloat = 12

    let title: String
    let more: Bool
    let seeAll: () -> Void

    var body: some View {
        HStack {
            Text(title)
            Spacer()
            if more {
                Button("search.see-all", action: seeAll)
                    .accessibilityLabel(said("search.see-all.spoken", title.lowercased()))
                    .font(.subheadline)
                    .textCase(nil)
                    .frame(minWidth: 44)
                    .padding(.vertical, Self.reachBeyondTheLine)
                    .contentShape(.rect)
                    .padding(.vertical, -Self.reachBeyondTheLine)
            }
        }
    }
}

struct NoneMatch: View {
    let kind: String
    let query: String
    let symbol: String

    var body: some View {
        ContentUnavailableView(
            titled(said("common.quoted", query)),
            systemImage: symbol,
            description: Text("search.none.hint")
        )
        .listRowSeparator(.hidden)
        .announced(titled(query))
    }

    private func titled(_ query: String) -> String {
        switch kind {
        case "albums": said("search.none.albums", query)
        case "artists": said("search.none.artists", query)
        case "favourites": said("search.none.favourites", query)
        default: said("search.none.songs", query)
        }
    }
}

struct NothingFound: View {
    let search: SearchModel

    var body: some View {
        if let meant = search.found.didYouMean {
            Button { search.text = meant } label: {
                Label {
                    Text(Self.meaning(meant))
                } icon: {
                    Image(systemName: "text.magnifyingglass")
                        .accessibilityHidden(true)
                }
            }
        }
        NoneMatch(kind: "songs", query: search.answered, symbol: "magnifyingglass")
    }

    static func meaning(_ meant: String) -> AttributedString {
        var asked = AttributedString(said("search.did-you-mean", meant))
        if let shown = asked.range(of: meant) {
            asked[shown].inlinePresentationIntent = .stronglyEmphasized
        }
        return asked
    }
}

struct SearchReadings: View {
    let reads: [String]

    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 8) {
                Image(systemName: "line.3.horizontal.decrease")
                    .foregroundStyle(.secondary)
                    .accessibilityHidden(true)
                ForEach(reads.indices, id: \.self) { index in
                    Text(reads[index])
                        .font(.footnote.monospaced())
                        .padding(.horizontal, 10)
                        .padding(.vertical, 5)
                        .glassEffect(.regular, in: .capsule)
                }
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 2)
        }
        .listRowInsets(EdgeInsets())
        .listRowSeparator(.hidden)
        .accessibilityElement(children: .combine)
        .accessibilityLabel(said("search.reads.spoken", reads.joined(separator: ", ")))
    }
}

struct SearchSuggestions: View {
    let search: SearchModel

    var body: some View {
        if search.query.isEmpty, !search.recents.isEmpty {
            Section("search.recents") {
                ForEach(search.recents, id: \.self) { recent in
                    Label(recent, systemImage: "clock.arrow.circlepath").searchCompletion(recent)
                }
                Button("search.recents.clear", role: .destructive) { search.forgetRecents() }
            }
        }
    }
}

struct SearchCompletions: View {
    private static let shown = 5

    let search: SearchModel

    var body: some View {
        let completions = Array(search.offeredCompletions.prefix(Self.shown))
        if !completions.isEmpty {
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 8) {
                    ForEach(completions, id: \.self) { completion in
                        Button { search.text = completion } label: {
                            Label(completion, systemImage: "magnifyingglass")
                                .font(.subheadline)
                                .titleLines()
                                .padding(.horizontal, 12)
                                .padding(.vertical, 8)
                                .glassEffect(.regular.interactive(), in: .capsule)
                                .contentShape(.capsule)
                        }
                        .buttonStyle(.plain)
                        .accessibilityLabel(said("search.completion.spoken", completion))
                    }
                }
            }
            .contentMargins(.horizontal, 16, for: .scrollContent)
            .listRowInsets(EdgeInsets(top: 6, leading: 0, bottom: 2, trailing: 0))
            .listRowSeparator(.hidden)
        }
    }
}

struct AlbumShelf: View {
    @Environment(Library.self) private var library
    @ScaledMetric(relativeTo: .subheadline) private var edge: CGFloat = 140
    let albums: [AlbumHit]
    let query: String
    var picked: () -> Void = {}

    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(alignment: .top, spacing: 14) {
                ForEach(albums, id: \.key) { album in
                    NavigationLink(value: album.destination) {
                        VStack(alignment: .leading, spacing: 4) {
                            ArtworkView(track: library.artworkTrack(album.artworkTrack), corner: 10)
                                .frame(width: edge, height: edge)
                            Text(lit(album.shownTitle, by: query, in: .album))
                                .font(.subheadline.weight(.medium))
                                .titleLines()
                            Text(lit(album.shownArtist, by: query, in: .artist))
                                .font(.caption)
                                .foregroundStyle(.secondary)
                                .titleLines()
                        }
                        .frame(width: edge, alignment: .leading)
                        .contentShape(.rect)
                    }
                    .buttonStyle(.plain)
                    .simultaneousGesture(TapGesture().onEnded { picked() })
                    .collectionActions(.album(album.key), artist: album.artist)
                }
            }
            .padding(.horizontal, 16)
        }
        .listRowInsets(EdgeInsets(top: 8, leading: 0, bottom: 8, trailing: 0))
        .listRowSeparator(.hidden)
    }
}

struct ArtistShelf: View {
    @Environment(Library.self) private var library
    @ScaledMetric(relativeTo: .subheadline) private var edge: CGFloat = 96
    let artists: [ArtistHit]
    let query: String
    var picked: () -> Void = {}

    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(alignment: .top, spacing: 16) {
                ForEach(artists, id: \.name) { artist in
                    NavigationLink(value: artist.destination) {
                        VStack(spacing: 6) {
                            ArtworkView(track: library.artworkTrack(artist.artworkTrack), circle: true)
                                .frame(width: edge, height: edge)
                            Text(lit(artist.shownName, by: query, in: .artist))
                                .font(.subheadline.weight(.medium))
                                .titleLines()
                            Text(said("counted.songs", Int(artist.songs)))
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                        .frame(width: edge + 8)
                        .contentShape(.rect)
                    }
                    .buttonStyle(.plain)
                    .simultaneousGesture(TapGesture().onEnded { picked() })
                    .collectionActions(.artist(artist.name))
                }
            }
            .padding(.horizontal, 16)
        }
        .listRowInsets(EdgeInsets(top: 8, leading: 0, bottom: 8, trailing: 0))
        .listRowSeparator(.hidden)
    }
}
