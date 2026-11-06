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
                    "Your library is empty",
                    systemImage: "music.note.house",
                    description: Text("Add a music folder in Settings to fill your home with mixes, albums and artists.")
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
                        SeeAllPageHeader(title: "Recently added", more: true, page: .albums)
                    }
                }
            }
            HomeDevices()
            Section("Listen together") {
                NavigationLink(value: Destination.party) {
                    ToolRow(title: "Listening party", detail: "Start or join a party with other people", symbol: "person.2.wave.2")
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
                HomeListeningSections()
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

private struct HomeDevices: View {
    @Environment(Library.self) private var library

    var body: some View {
        let features = library.features
        let devices = features.syncStatus?.devices.filter { !$0.thisDevice && ($0.online || $0.nearby) } ?? []
        Section("Your devices") {
            ForEach(devices, id: \.id) { device in
                KnownDeviceEntry(known: device)
            }
            NavigationLink(value: Destination.sync) {
                ToolRow(title: "Remote control", detail: "Pair devices and control their playback", symbol: "appletvremote")
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
                title: "Shuffle all",
                detail: counted(library.catalogCount, "song"),
                symbol: "shuffle",
                from: .indigo,
                to: .blue
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
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(alignment: .top, spacing: 14) {
                    ForEach(offered.home) { mix in
                        SuggestedMixCard(
                            mix: mix,
                            covers: (offered.covers[mix.id] ?? []).compactMap { library.artworkTrack($0) },
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
        .task(id: "\(library.collectionGeneration):\(library.catalogEpoch)") {
            guard let found = await OfferedMixes.read(in: library) else { return }
            offered = found
        }
    }
}
