import ResonateLogic
import ResonateNative
import SwiftUI

extension AlbumHit {
    var destination: Destination { .album(key) }
}

extension ArtistHit {
    var destination: Destination { .artist(name) }
}

struct SearchResults: View {
    private static let topSongs = 5
    private static let topCollections = 6

    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    let search: SearchModel
    @Binding var scope: SearchScope
    var chosen: () -> Void = {}

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
            ForEach(found.tracks.prefix(Self.topSongs), id: \.id) { track in
                songRow(track, among: found.tracks, query)
            }
        } header: {
            SeeAllHeader(title: "Songs", more: found.trackCount > Self.topSongs) { scope = .songs }
        }
        if !found.albums.isEmpty {
            Section {
                AlbumShelf(albums: Array(found.albums.prefix(Self.topCollections)), query: query, picked: picked)
            } header: {
                SeeAllHeader(title: "Albums", more: found.albums.count > Self.topCollections) { scope = .albums }
            }
        }
        if !found.artists.isEmpty {
            Section {
                ArtistShelf(artists: Array(found.artists.prefix(Self.topCollections)), query: query, picked: picked)
            } header: {
                SeeAllHeader(title: "Artists", more: found.artists.count > Self.topCollections) { scope = .artists }
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
            Text(counted(Int(found.trackCount), "song"))
        } footer: {
            if found.trackCount > found.tracks.count {
                Text("Showing the best \(found.tracks.count). Add a word to narrow the search.")
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
            if library.snapshot?.shuffle == true, !library.allTracks.isEmpty {
                library.play(library.allTracks, at: library.allTracks.firstIndex { $0.id == track.id } ?? 0, shuffle: true)
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
                Button("See all", action: seeAll)
                    .accessibilityLabel("See all \(title.lowercased())")
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
            "No \(kind) match “\(query)”",
            systemImage: symbol,
            description: Text("Check the spelling, or try fewer words.")
        )
        .listRowSeparator(.hidden)
    }
}

struct NothingFound: View {
    let search: SearchModel

    var body: some View {
        if let meant = search.found.didYouMean {
            Button { search.text = meant } label: {
                Label {
                    Text("Did you mean \(Text(meant).bold())?")
                } icon: {
                    Image(systemName: "text.magnifyingglass")
                }
            }
        }
        NoneMatch(kind: "songs", query: search.answered, symbol: "magnifyingglass")
    }
}

struct SearchReadings: View {
    let reads: [String]

    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 8) {
                Image(systemName: "line.3.horizontal.decrease")
                    .foregroundStyle(.secondary)
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
        .accessibilityLabel("Searching for \(reads.joined(separator: ", "))")
    }
}

struct SearchSuggestions: View {
    let search: SearchModel

    var body: some View {
        if search.query.isEmpty, !search.recents.isEmpty {
            Section("Recent searches") {
                ForEach(search.recents, id: \.self) { recent in
                    Label(recent, systemImage: "clock.arrow.circlepath").searchCompletion(recent)
                }
                Button("Clear recent searches", role: .destructive) { search.forgetRecents() }
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
                                .lineLimit(1)
                                .padding(.horizontal, 12)
                                .padding(.vertical, 8)
                                .glassEffect(.regular.interactive(), in: .capsule)
                                .contentShape(.capsule)
                        }
                        .buttonStyle(.plain)
                        .accessibilityLabel("Search for \(completion)")
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
                                .lineLimit(1)
                            Text(lit(album.shownArtist, by: query, in: .artist))
                                .font(.caption)
                                .foregroundStyle(.secondary)
                                .lineLimit(1)
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
                                .lineLimit(1)
                            Text(counted(Int(artist.songs), "song"))
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
