import ResonateLogic
import ResonateNative
import SwiftUI

struct LibraryAlbums: View {
    @Environment(Library.self) private var library
    @Environment(\.dynamicTypeSize) private var typeSize

    var body: some View {
        if library.librarySearch.isAsked {
            LibraryFound(scope: .albums)
        } else {
            switch AlbumsLayout.shown(asRows: library.features.albumsAsRows, accessibilityText: typeSize.isAccessibilitySize) {
            case .grid: AlbumGrid()
            case .rows: AlbumRows()
            }
        }
    }
}

private struct AlbumsHeader: View {
    @Environment(Library.self) private var library
    @Environment(\.dynamicTypeSize) private var typeSize

    var body: some View {
        let asRows = library.features.albumsAsRows
        ShelfHeader(title: said("counted.albums", library.albums.count)) {
            CollectionOrderMenu(shelf: .albums)
            if !typeSize.isAccessibilitySize {
                Button {
                    library.features.showAlbums(asRows: !asRows)
                } label: {
                    Label(said(asRows ? "library.albums.as-grid" : "library.albums.as-rows"), systemImage: asRows ? "square.grid.2x2" : "list.bullet")
                        .labelStyle(.iconOnly)
                        .font(.subheadline.weight(.medium))
                        .touchTarget()
                }
                .buttonStyle(.plain)
            }
        }
    }
}

private struct AlbumsStanding: View {
    var body: some View {
        ShelfStanding(none: said("library.albums.none"), detail: said("library.albums.none.detail"), symbol: "square.stack")
    }
}

struct AlbumGrid: View {
    private static let narrowest: CGFloat = 150
    private static let between: CGFloat = 12

    @Environment(Library.self) private var library
    @State private var track = ScrollTrack()

    var body: some View {
        let albums = library.albums
        ScrollViewReader { reader in
            ScrollView {
                VStack(alignment: .leading, spacing: 12) {
                    AlbumsHeader()
                    if albums.isEmpty {
                        AlbumsStanding()
                    } else {
                        LazyVGrid(
                            columns: [GridItem(.adaptive(minimum: Self.narrowest), spacing: Self.between, alignment: .top)],
                            spacing: 16
                        ) {
                            ForEach(albums, id: \.key) { AlbumCell(album: $0) }
                        }
                    }
                }
                .padding(.horizontal, 16)
                .padding(.vertical, 8)
            }
            .revealsLibrarySearch()
            .onScrollGeometryChange(for: Double.self) { geometry in
                ScrollTrack.share(of: geometry)
            } action: { _, share in
                track.moved(to: share)
            }
            .overlay(alignment: .trailing) {
                FastScroller(track: track, rows: albums.count, window: 0..<albums.count) { row in
                    guard row < albums.count else { return }
                    reader.scrollTo(albums[row].key, anchor: .top)
                }
            }
        }
    }
}

struct AlbumCell: View {
    @Environment(Library.self) private var library
    let album: AlbumHit

    var body: some View {
        NavigationLink(value: album.destination) {
            VStack(alignment: .leading, spacing: 4) {
                Color.clear
                    .aspectRatio(1, contentMode: .fit)
                    .overlay { ArtworkView(track: library.artworkTrack(album.artworkTrack), corner: 12) }
                Text(album.shownTitle)
                    .font(.subheadline.weight(.medium))
                    .titleLines()
                Text(album.byline)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .titleLines()
            }
            .contentShape(.rect)
            .accessibilityElement(children: .combine)
        }
        .buttonStyle(.plain)
        .collectionActions(.album(album.key), artist: album.variousArtists ? nil : album.artist)
    }
}

struct AlbumRows: View {
    @Environment(Library.self) private var library

    var body: some View {
        let albums = library.albums
        List {
            AlbumsHeader()
                .listRowSeparator(.hidden)
                .themedRows()
            Group {
                if albums.isEmpty {
                    AlbumsStanding().listRowSeparator(.hidden)
                } else if library.albumRuns.isEmpty {
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
            .themedRows()
        }
        .listStyle(.plain)
        .revealsLibrarySearch()
        .listSectionIndexVisibility(.visible)
    }

    private func link(_ album: AlbumHit) -> some View {
        NavigationLink(value: album.destination) {
            CollectionRow(track: library.artworkTrack(album.artworkTrack), title: album.shownTitle, detail: album.detail)
        }
        .collectionActions(.album(album.key), artist: album.variousArtists ? nil : album.artist)
    }
}
