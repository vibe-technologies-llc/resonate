import ResonateNative
import SwiftUI

extension AlbumHit {
    var shownTitle: String { title.isEmpty ? "Unknown album" : title }
    var shownArtist: String { artist.isEmpty ? "Unknown artist" : artist }
    var destination: Destination { .album(key) }

    var detail: String {
        [shownArtist, year.map(String.init) ?? "", counted(Int(songs), "song")]
            .filter { !$0.isEmpty }
            .joined(separator: " · ")
    }
}

extension ArtistHit {
    var shownName: String { name.isEmpty ? "Unknown artist" : name }
    var destination: Destination { .artist(name) }

    var detail: String {
        albums > 0 ? "\(counted(Int(albums), "album")) · \(counted(Int(songs), "song"))" : counted(Int(songs), "song")
    }
}

struct SearchResults: View {
    private static let topSongs = 5
    private static let topCollections = 6

    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    let search: SearchModel
    @Binding var scope: SearchScope

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
                AlbumShelf(albums: Array(found.albums.prefix(Self.topCollections)), query: query) { open($0.destination) }
            } header: {
                SeeAllHeader(title: "Albums", more: found.albums.count > Self.topCollections) { scope = .albums }
            }
        }
        if !found.artists.isEmpty {
            Section {
                ArtistShelf(artists: Array(found.artists.prefix(Self.topCollections)), query: query) { open($0.destination) }
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
                FoundCollectionRow(
                    track: library.artworkTrack(album.artworkTrack),
                    title: lit(album.shownTitle, by: query, in: .album),
                    detail: album.detail
                )
            }
            .buttonStyle(.plain)
        }
    }

    @ViewBuilder
    private func artists(_ found: LibrarySearch, _ query: String) -> some View {
        if found.artists.isEmpty {
            NoneMatch(kind: "artists", query: query, symbol: "music.mic")
        }
        ForEach(found.artists, id: \.name) { artist in
            Button { open(artist.destination) } label: {
                FoundCollectionRow(
                    track: library.artworkTrack(artist.artworkTrack),
                    title: lit(artist.shownName, by: query, in: .artist),
                    detail: artist.detail,
                    circle: true
                )
            }
            .buttonStyle(.plain)
        }
    }

    private func songRow(_ track: Track, among tracks: [Track], _ query: String) -> some View {
        FoundTrackRow(track: track, query: query, current: library.snapshot?.current?.id == track.id) {
            search.remember()
            library.play(tracks, selected: track)
        }
    }

    private func open(_ destination: Destination) {
        search.remember()
        navigation.open(destination)
    }
}

struct SeeAllHeader: View {
    let title: String
    let more: Bool
    let seeAll: () -> Void

    var body: some View {
        HStack {
            Text(title)
            Spacer()
            if more {
                Button("See all", action: seeAll)
                    .font(.subheadline)
                    .textCase(nil)
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
    private static let completionsShown = 5

    let search: SearchModel
    var offersRecents = true

    var body: some View {
        if search.query.isEmpty {
            if offersRecents, !search.recents.isEmpty {
                Section("Recent searches") {
                    ForEach(search.recents, id: \.self) { recent in
                        Label(recent, systemImage: "clock.arrow.circlepath").searchCompletion(recent)
                    }
                    Button("Clear recent searches", role: .destructive) { search.forgetRecents() }
                }
            }
        } else {
            ForEach(search.offeredCompletions.prefix(Self.completionsShown), id: \.self) { completion in
                Label(completion, systemImage: "magnifyingglass").searchCompletion(completion)
            }
        }
    }
}

struct FoundTrackRow: View {
    @Environment(Navigation.self) private var navigation
    let track: Track
    let query: String
    var current = false
    let play: () -> Void
    @State private var choosingPlaylist = false

    var body: some View {
        HStack(spacing: 12) {
            Button(action: play) {
                HStack(spacing: 12) {
                    ArtworkView(track: track, corner: 8).frame(width: 48, height: 48)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(lit(track.shownTitle, by: query, in: .title))
                            .font(.body.weight(current ? .semibold : .regular))
                            .foregroundStyle(current ? Color.accentColor : Color.primary)
                            .lineLimit(1)
                        Text(detail)
                            .font(.subheadline)
                            .foregroundStyle(.secondary)
                            .lineLimit(1)
                    }
                    Spacer(minLength: 0)
                }
                .contentShape(.rect)
            }
            .buttonStyle(.plain)
            FavouriteButton(track: track)
        }
        .contextMenu { TrackMenu(track: track, choosingPlaylist: $choosingPlaylist) }
        .sheet(isPresented: $choosingPlaylist) { PlaylistPicker(tracks: [track]) }
        .accessibilityAction(named: "Song details") { navigation.open(.inspector(track)) }
    }

    private var detail: AttributedString {
        var shown = lit(track.shownArtist, by: query, in: .artist)
        if !track.album.isEmpty {
            shown.append(AttributedString(" · "))
            shown.append(lit(track.album, by: query, in: .album))
        }
        return shown
    }
}

struct FoundCollectionRow: View {
    let track: Track?
    let title: AttributedString
    let detail: String
    var circle = false
    var disclosed = true

    var body: some View {
        HStack(spacing: 14) {
            ArtworkView(track: track, corner: 10, circle: circle)
                .frame(width: 56, height: 56)
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
    }
}

struct AlbumShelf: View {
    @Environment(Library.self) private var library
    let albums: [AlbumHit]
    let query: String
    let open: (AlbumHit) -> Void

    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(alignment: .top, spacing: 14) {
                ForEach(albums, id: \.key) { album in
                    Button { open(album) } label: {
                        VStack(alignment: .leading, spacing: 4) {
                            ArtworkView(track: library.artworkTrack(album.artworkTrack), corner: 12)
                                .frame(width: 140, height: 140)
                            Text(lit(album.shownTitle, by: query, in: .album))
                                .font(.subheadline.weight(.medium))
                                .lineLimit(1)
                            Text(lit(album.shownArtist, by: query, in: .artist))
                                .font(.caption)
                                .foregroundStyle(.secondary)
                                .lineLimit(1)
                        }
                        .frame(width: 140, alignment: .leading)
                        .contentShape(.rect)
                    }
                    .buttonStyle(.plain)
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
    let artists: [ArtistHit]
    let query: String
    let open: (ArtistHit) -> Void

    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(alignment: .top, spacing: 16) {
                ForEach(artists, id: \.name) { artist in
                    Button { open(artist) } label: {
                        VStack(spacing: 6) {
                            ArtworkView(track: library.artworkTrack(artist.artworkTrack), circle: true)
                                .frame(width: 96, height: 96)
                            Text(lit(artist.shownName, by: query, in: .artist))
                                .font(.subheadline.weight(.medium))
                                .lineLimit(1)
                            Text(counted(Int(artist.songs), "song"))
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                        .frame(width: 104)
                        .contentShape(.rect)
                    }
                    .buttonStyle(.plain)
                }
            }
            .padding(.horizontal, 16)
        }
        .listRowInsets(EdgeInsets(top: 8, leading: 0, bottom: 8, trailing: 0))
        .listRowSeparator(.hidden)
    }
}
