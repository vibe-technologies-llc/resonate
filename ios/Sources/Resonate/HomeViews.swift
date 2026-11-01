import ResonateLogic
import ResonateNative
import SwiftUI

struct HomeView: View {
    private static let shelved = 12

    @Environment(Library.self) private var library
    @Environment(\.scenePhase) private var phase
    @State private var hour = Calendar.current.component(.hour, from: .now)
    @State private var recent: [AlbumHit] = []
    @State private var listenedTo: [ArtistHit] = []
    @State private var naming = false

    var body: some View {
        let features = library.features
        let playlists = features.playlists
        let empty = library.catalogRead && library.catalogCount == 0
        ThemedList {
            if empty {
                ContentUnavailableView(
                    "Your library is empty",
                    systemImage: "music.note.house",
                    description: Text("Add a music folder in Settings to fill your home with mixes, albums and artists.")
                )
                .listRowBackground(Color.clear)
            } else {
                Section {
                    NavigationLink(value: Destination.favourites) {
                        FavouritesCountRow()
                    }
                    ShuffleAllRow()
                }
                HomeMixes()
                if !recent.isEmpty {
                    Section {
                        AlbumShelf(albums: recent, query: "")
                    } header: {
                        SeeAllPageHeader(title: "Recently added", more: true, page: .albums)
                    }
                }
            }
            Section {
                if playlists.isEmpty {
                    Button {
                        naming = true
                    } label: {
                        Label("New playlist", systemImage: "plus")
                    }
                } else {
                    PlaylistShelf(playlists: Array(playlists.prefix(Self.shelved)))
                }
            } header: {
                SeeAllPageHeader(title: "Your playlists", more: !playlists.isEmpty, page: .playlists)
            }
            if !empty {
                if !listenedTo.isEmpty {
                    Section {
                        ArtistShelf(artists: listenedTo, query: "")
                    } header: {
                        SeeAllPageHeader(title: "Artists", more: true, page: .artists)
                    }
                }
                Section("Your listening") {
                    NavigationLink(value: Destination.statistics) {
                        ListeningSummary()
                    }
                }
            }
        }
        .listStyle(.insetGrouped)
        .navigationTitle(HomeGreeting.of(hour: hour))
        .refreshable {
            library.rescan()
            features.loadPlaylists()
            features.loadStatistics()
        }
        .toolbar { LibraryToolbar() }
        .onAppear {
            hour = Calendar.current.component(.hour, from: .now)
            features.loadPlaylists()
            features.loadStatistics()
        }
        .onChange(of: phase) { _, now in
            if now == .active { hour = Calendar.current.component(.hour, from: .now) }
        }
        .task(id: "\(library.collectionGeneration):\(library.catalogEpoch)") { await loadShelves() }
        .alert("Create playlist", isPresented: $naming) {
            PlaylistNameField(confirm: "Create") { features.createPlaylist($0) }
        }
    }

    private func loadShelves() async {
        guard let native = try? await library.player() else { return }
        let shelved = Self.shelved
        guard let (albums, artists) = try? await Work.onScreen({
            (
                Array(try native.albums(order: .added).prefix(shelved)),
                Array(try native.artists(order: .plays).prefix(shelved))
            )
        }) else { return }
        recent = albums
        listenedTo = artists
    }
}

private struct ShuffleAllRow: View {
    @Environment(Library.self) private var library
    @ScaledMetric(relativeTo: .body) private var artworkEdge: CGFloat = 56

    var body: some View {
        Button {
            library.shuffle(library.allTracks)
        } label: {
            HStack(spacing: 14) {
                ShelfTile(symbol: "shuffle", from: .orange, to: .red)
                    .frame(width: artworkEdge, height: artworkEdge)
                VStack(alignment: .leading, spacing: 2) {
                    Text("Shuffle all").font(.body.weight(.medium))
                    Text(counted(library.catalogCount, "song")).font(.subheadline).foregroundStyle(.secondary)
                }
                Spacer(minLength: 0)
            }
            .contentShape(.rect)
            .accessibilityElement(children: .combine)
        }
        .buttonStyle(.plain)
        .disabled(library.catalogCount == 0)
    }
}

private struct HomeMixes: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    @ScaledMetric(relativeTo: .subheadline) private var cardWidth: CGFloat = 160
    @State private var covers: [String: [UInt64]] = [:]

    var body: some View {
        let mixes = SuggestedMix.offered(topArtist: library.features.listeningDetail?.artists.first?.name)
        Section {
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(alignment: .top, spacing: 14) {
                    ForEach(mixes) { mix in
                        SuggestedMixCard(
                            mix: mix,
                            covers: (covers[mix.id] ?? []).compactMap { library.artworkTrack($0) },
                            open: { navigation.open(.mix(mix.id)) }
                        )
                        .frame(width: cardWidth)
                    }
                }
                .padding(.horizontal, 16)
            }
            .listRowInsets(EdgeInsets(top: 8, leading: 0, bottom: 8, trailing: 0))
            .listRowSeparator(.hidden)
        } header: {
            SeeAllPageHeader(title: "Made for you", more: true, page: .suggestions)
        }
        .task(id: "\(library.collectionGeneration):\(mixes.map(\.id).joined())") {
            guard let found = await SuggestedMix.covers(of: mixes, in: library) else { return }
            covers = found
        }
    }
}

private struct ListeningSummary: View {
    @Environment(Library.self) private var library

    var body: some View {
        let features = library.features
        let statistics = features.statisticsDays == 0 ? features.statistics : nil
        HStack(alignment: .top, spacing: 12) {
            ListeningFigure(value: statistics.map { "\($0.plays)" } ?? "–", label: "plays")
            ListeningFigure(value: statistics.map { StatisticsView.brief($0.listenedMs) } ?? "–", label: "listened")
            ListeningFigure(value: "\(statistics.map { Int($0.songs) } ?? library.catalogCount)", label: "songs")
        }
        .padding(.vertical, 6)
        .accessibilityElement(children: .combine)
        .accessibilityHint("Shows your listening statistics")
    }
}

private struct ListeningFigure: View {
    let value: String
    let label: String

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(value)
                .font(.title2.weight(.semibold))
                .foregroundStyle(.tint)
                .lineLimit(1)
                .minimumScaleFactor(0.6)
            Text(label)
                .font(.caption)
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}
