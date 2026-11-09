import ResonateLogic
import ResonateNative
import SwiftUI

struct HomeView: View {
    private static let shelved = 12

    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    @Environment(\.scenePhase) private var phase
    @State private var wentToBackground = false
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
                    "home.empty",
                    systemImage: "music.note.house",
                    description: Text("home.empty.detail")
                )
                .listRowBackground(Color.clear)
            } else {
                Section {
                    ShortcutCards {
                        NavigationLink(value: Destination.favourites) {
                            FavouritesCard()
                        }
                        .buttonStyle(.plain)
                        ShuffleAllCard()
                    }
                }
                HomeMixes()
                if !recent.isEmpty {
                    Section {
                        AlbumShelf(albums: recent, query: "")
                    } header: {
                        SeeAllPageHeader(title: said("collections.order.added"), more: true, page: .albums)
                    }
                }
            }
            Section {
                if playlists.isEmpty {
                    Button {
                        naming = true
                    } label: {
                        Label("playlists.new", systemImage: "plus")
                    }
                } else {
                    PlaylistShelf(playlists: Array(playlists.prefix(Self.shelved)))
                }
            } header: {
                SeeAllPageHeader(title: said("home.playlists"), more: !playlists.isEmpty, page: .playlists)
            }
            if !empty {
                if !listenedTo.isEmpty {
                    Section {
                        ArtistShelf(artists: listenedTo, query: "")
                    } header: {
                        SeeAllPageHeader(title: said("library.artists"), more: true, page: .artists)
                    }
                }
                HomeListeningSections()
            }
            HomeDevices()
            Section("home.together") {
                NavigationLink(value: Destination.party) {
                    ToolRow(title: said("tools.party"), detail: said("home.party.detail"), symbol: "person.2.wave.2")
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
            if now == .background { wentToBackground = true }
            guard now == .active else { return }

            hour = Calendar.current.component(.hour, from: .now)
            if wentToBackground, navigation.tab == .home, navigation.paths[.home]?.isEmpty ?? true {
                features.loadStatistics()
            }
            wentToBackground = false
        }
        .background { CatalogReading { await loadShelves() } }
        .alert("playlists.create", isPresented: $naming) {
            PlaylistNameField(confirm: said("common.create")) { features.createPlaylist($0) }
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
        if recent != albums { recent = albums }
        if listenedTo != artists { listenedTo = artists }
    }
}

private struct HomeDevices: View {
    @Environment(Library.self) private var library

    var body: some View {
        let features = library.features
        let devices = features.syncStatus?.devices.filter { !$0.thisDevice && ($0.online || $0.nearby) } ?? []
        Section("playlist.devices") {
            ForEach(devices, id: \.id) { device in
                KnownDeviceEntry(known: device)
            }
            NavigationLink(value: Destination.sync) {
                ToolRow(title: said("home.remote"), detail: said("home.remote.detail"), symbol: "appletvremote.gen4")
            }
            if !features.syncHandoffs.isEmpty {
                HandoffRows()
            }
        }
    }
}

private struct ShuffleAllCard: View {
    @Environment(Library.self) private var library

    var body: some View {
        Button {
            library.shuffle(library.allTracks)
        } label: {
            ShortcutCard(
                title: said("home.shuffle-all"),
                detail: said("counted.songs", library.catalogCount),
                symbol: "shuffle"
            )
        }
        .buttonStyle(.plain)
        .disabled(library.catalogCount == 0)
    }
}

private struct HomeMixes: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    @ScaledMetric(relativeTo: .subheadline) private var cardWidth: CGFloat = 160
    @State private var offered = OfferedMixes()

    var body: some View {
        Section {
            Shelf(spacing: 14) {
                ForEach(offered.home) { mix in
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
        .background {
            CatalogReading {
                guard let found = await OfferedMixes.read(in: library), found != offered else { return }
                offered = found
            }
        }
    }
}
