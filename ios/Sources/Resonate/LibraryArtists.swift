import ResonateLogic
import ResonateNative
import SwiftUI

struct LibraryArtists: View {
    @Environment(Library.self) private var library

    var body: some View {
        if library.librarySearch.isAsked {
            LibraryFound(scope: .artists)
        } else {
            ArtistRows()
        }
    }
}

struct ArtistRows: View {
    @Environment(Library.self) private var library
    @State private var track = ScrollTrack()

    var body: some View {
        let artists = library.artists
        ScrollViewReader { reader in
            List {
                ShelfHeader(title: said("counted.artists", artists.count)) {
                    CollectionOrderMenu(shelf: .artists)
                }
                .listRowSeparator(.hidden)
                .themedRows()
                Group {
                    if artists.isEmpty {
                        ShelfStanding(none: said("library.artists.none"), detail: said("library.artists.none.detail"), symbol: "music.mic")
                            .listRowSeparator(.hidden)
                    } else if library.artistRuns.isEmpty {
                        ForEach(artists, id: \.name) { link($0) }
                    } else {
                        ForEach(library.artistRuns) { run in
                            Section {
                                ForEach(artists[artists.clamped(run.rows)], id: \.name) { link($0) }
                            } header: {
                                Text(run.letter)
                            }
                        }
                    }
                }
                .themedRows()
            }
            .listStyle(.plain)
            .listSectionIndexVisibility(.hidden)
            .onScrollGeometryChange(for: Double.self) { geometry in
                ScrollTrack.share(of: geometry)
            } action: { _, share in
                track.moved(to: share)
            }
            .overlay(alignment: .trailing) {
                FastScroller(track: track, rows: artists.count, window: 0..<artists.count) { row in
                    guard row < artists.count else { return }
                    reader.scrollTo(artists[row].name, anchor: .top)
                }
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
