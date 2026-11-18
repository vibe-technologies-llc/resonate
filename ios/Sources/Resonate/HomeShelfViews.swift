import ResonateLogic
import ResonateNative
import SwiftUI

struct HomeJumpBackIn: View {
    private static let between: CGFloat = 10

    let tiles: [JumpBackTile]
    let columns: Int

    var body: some View {
        let rows = stride(from: 0, to: tiles.count, by: columns).map { Array(tiles[$0..<min($0 + columns, tiles.count)]) }
        Section("home.jump-back-in") {
            ScrollView(.horizontal) {
                VStack(spacing: Self.between) {
                    ForEach(rows, id: \.first?.id) { row in
                        HStack(spacing: Self.between) {
                            ForEach(row) { JumpBackTileLink(tile: $0) }
                            ForEach(row.count..<columns, id: \.self) { _ in
                                Color.clear
                                    .frame(maxWidth: .infinity)
                                    .accessibilityHidden(true)
                            }
                        }
                        .fixedSize(horizontal: false, vertical: true)
                    }
                }
                .containerRelativeFrame(.horizontal)
            }
            .scrollDisabled(true)
            .scrollIndicators(.hidden)
            .listRowInsets(EdgeInsets())
            .listRowBackground(Color.clear)
            .listRowSeparator(.hidden)
        }
    }
}

struct JumpBackTileLink: View {
    @Environment(Library.self) private var library
    @ScaledMetric(relativeTo: .subheadline) private var edge: CGFloat = 56
    let tile: JumpBackTile

    var body: some View {
        switch tile.subject {
        case .album(let key, let artist): link.collectionActions(.album(key), artist: artist)
        case .favourites: link
        }
    }

    private var link: some View {
        NavigationLink(value: tile.destination) {
            HStack(spacing: 10) {
                cover
                    .frame(width: edge, height: edge)
                Text(tile.title)
                    .font(.subheadline.weight(.semibold))
                    .multilineTextAlignment(.leading)
                    .titleLines(2)
                Spacer(minLength: 0)
            }
            .padding(.trailing, 10)
        }
        .buttonStyle(.jumpBackTile)
        .accessibilityLabel(tile.spoken)
    }

    @ViewBuilder
    private var cover: some View {
        if let song = tile.cover {
            ArtworkView(track: library.artworkTrack(song.id) ?? song, corner: 0)
        } else {
            ShelfTile(symbol: "heart.fill", corner: 0)
        }
    }
}

struct JumpBackTileStyle: ButtonStyle {
    private static let corner: CGFloat = 12

    func makeBody(configuration: Configuration) -> some View {
        let shape = RoundedRectangle(cornerRadius: Self.corner, style: .continuous)
        configuration.label
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .leading)
            .background(Color.cardGround)
            .clipShape(shape)
            .contentShape(shape)
            .opacity(configuration.isPressed ? 0.7 : 1)
    }
}

extension ButtonStyle where Self == JumpBackTileStyle {
    static var jumpBackTile: JumpBackTileStyle { JumpBackTileStyle() }
}

struct HomeMixes: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    @ScaledMetric(relativeTo: .subheadline) private var cardWidth: CGFloat = 160
    let offered: OfferedMixes

    var body: some View {
        let mixes = offered.home
        if !mixes.isEmpty {
            Section {
                Shelf(spacing: 14) {
                    ForEach(mixes) { mix in
                        SuggestedMixCard(
                            mix: mix,
                            covers: (offered.covers[mix.id] ?? []).compactMap { library.artworkTrack($0) },
                            shelved: true,
                            open: { navigation.open(.mix(mix.id)) }
                        )
                        .frame(width: cardWidth)
                    }
                }
                .listRowInsets(EdgeInsets(top: 8, leading: 0, bottom: 8, trailing: 0))
                .listRowSeparator(.hidden)
            } header: {
                SeeAllPageHeader(title: said("home.made-for-you"), more: true, page: .suggestions)
            }
        }
    }
}

struct HomeRecentlyAdded: View {
    private static let widest: CGFloat = 260

    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    @ScaledMetric(relativeTo: .title3) private var edge: CGFloat = 200
    let albums: [AlbumHit]

    var body: some View {
        if !albums.isEmpty {
            Section {
                HomeCarousel {
                    ForEach(albums, id: \.key) { album in
                        RecentAlbumCard(album: album, edge: min(edge, Self.widest))
                    }
                }
            } header: {
                SeeAllHeader(title: said("home.recently-added"), more: true) {
                    navigation.showLibrary(.albums(by: .added), in: library)
                }
            }
        }
    }
}

private struct HomeCarousel<Cards: View>: View {
    @Environment(\.dynamicTypeSize) private var typeSize
    @ViewBuilder let cards: Cards

    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            if typeSize.isAccessibilitySize {
                HStack(alignment: .top, spacing: 14) { cards }
                    .scrollTargetLayout()
            } else {
                LazyHStack(alignment: .top, spacing: 14) { cards }
                    .scrollTargetLayout()
            }
        }
        .scrollTargetBehavior(.viewAligned)
        .contentMargins(.horizontal, 16, for: .scrollContent)
        .listRowInsets(EdgeInsets(top: 8, leading: 0, bottom: 8, trailing: 0))
        .listRowSeparator(.hidden)
    }
}

struct RecentAlbumCard: View {
    @Environment(Library.self) private var library
    let album: AlbumHit
    let edge: CGFloat

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            ZStack(alignment: .bottomTrailing) {
                NavigationLink(value: album.destination) {
                    ArtworkView(track: library.artworkTrack(album.artworkTrack), corner: 16)
                        .frame(width: edge, height: edge)
                }
                .buttonStyle(.plain)
                .accessibilityHidden(true)
                PlayCollectionButton(named: album.shownTitle, source: .album(album.key))
                    .padding(10)
            }
            NavigationLink(value: album.destination) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(album.shownTitle)
                        .font(.subheadline.weight(.semibold))
                        .titleLines()
                    Text(album.byline)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .titleLines()
                }
                .frame(width: edge, alignment: .leading)
                .contentShape(.rect)
            }
            .buttonStyle(.plain)
        }
        .collectionActions(.album(album.key), artist: album.variousArtists ? nil : album.artist)
    }
}

struct PlayCollectionButton: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    let named: String
    let source: CollectionSource

    var body: some View {
        Button(said("mix.play.named", named), systemImage: "play.fill", action: play)
            .labelStyle(.iconOnly)
            .buttonStyle(.glass)
            .buttonBorderShape(.circle)
    }

    private func play() {
        let source = source
        Task {
            do {
                let native = try await library.player()
                let tracks = try await Work.onScreen { try source.tracks(native) }
                guard !tracks.isEmpty else { return }
                navigation.feelQueued()
                library.play(tracks, at: 0)
            } catch {
                library.report(error)
            }
        }
    }
}

struct HomePlayedArtists: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    @ScaledMetric(relativeTo: .subheadline) private var edge: CGFloat = 96
    let artists: [PlayedArtist]

    var body: some View {
        if !artists.isEmpty {
            Section {
                Shelf(spacing: 16) {
                    ForEach(artists) { artist in
                        NavigationLink(value: Destination.artist(artist.name)) {
                            VStack(spacing: 6) {
                                ArtworkView(track: artist.cover.map { library.artworkTrack($0.id) ?? $0 }, circle: true)
                                    .frame(width: edge, height: edge)
                                Text(artist.name.isEmpty ? said("library.unknown.artist") : artist.name)
                                    .font(.subheadline.weight(.medium))
                                    .titleLines()
                                Text(said("counted.plays", Int(artist.plays)))
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                            }
                            .frame(width: edge + 8)
                            .contentShape(.rect)
                        }
                        .buttonStyle(.plain)
                        .collectionActions(.artist(artist.name))
                    }
                }
                .listRowInsets(EdgeInsets(top: 8, leading: 0, bottom: 8, trailing: 0))
                .listRowSeparator(.hidden)
            } header: {
                SeeAllHeader(title: said("home.artists"), more: true) {
                    navigation.showLibrary(.artists(by: .plays), in: library)
                }
            }
        }
    }
}
