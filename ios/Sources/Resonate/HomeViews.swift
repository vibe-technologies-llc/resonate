import ResonateLogic
import ResonateNative
import SwiftUI

struct HomeView: View {
    @Environment(Library.self) private var library
    @Environment(\.scenePhase) private var phase
    @State private var hour = Calendar.current.component(.hour, from: .now)

    var body: some View {
        let features = library.features
        let feed = features.home
        let empty = library.catalogEmpty
        ThemedList {
            if empty {
                EmptyLibrary()
                    .listRowBackground(Color.clear)
            } else {
                HomeCatalogSections(feed: feed)
            }
            HomePlaylists(hiddenWithNone: empty)
            if !empty {
                HomeListeningGlance(feed: feed)
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
            await feed.read(afresh: true)
        }
        .toolbar { LibraryToolbar() }
        .onAppear {
            hour = Calendar.current.component(.hour, from: .now)
            features.loadPlaylists()
        }
        .onChange(of: phase) { _, now in
            if now == .active { hour = Calendar.current.component(.hour, from: .now) }
        }
        .background { HomeFeedReading(feed: feed) }
    }
}

private struct HomeFeedReading: View {
    @Environment(Navigation.self) private var navigation
    @Environment(\.scenePhase) private var phase
    let feed: HomeFeed
    @State private var wentToBackground = false

    var body: some View {
        CatalogReading { await feed.read() }
            .onChange(of: phase) { _, now in
                if now == .background { wentToBackground = true }
                guard now == .active, wentToBackground else { return }

                wentToBackground = false
                readWhereShown()
            }
            .onChange(of: navigation.nowPlaying) { _, open in
                if !open { readWhereShown() }
            }
    }

    private func readWhereShown() {
        guard navigation.tab == .home, navigation.paths[.home]?.isEmpty ?? true else { return }
        Task { await feed.read() }
    }
}

private struct HomeCatalogSections: View {
    @Environment(Library.self) private var library
    @Environment(\.horizontalSizeClass) private var horizontal
    @Environment(\.dynamicTypeSize) private var typeSize
    let feed: HomeFeed

    var body: some View {
        let columns = JumpBackGrid.columns(horizontal: SizeClass(horizontal), accessibilityText: typeSize.isAccessibilitySize)
        let tiles = JumpBackIn.shown(feed.played, favourites: !library.favouriteTracks.isEmpty, columns: columns)
        if tiles.isEmpty {
            HomeRecentlyAdded(albums: feed.recent)
            HomeMixes(offered: feed.mixes)
        } else {
            HomeJumpBackIn(tiles: tiles, columns: columns)
            HomeMixes(offered: feed.mixes)
            HomeRecentlyAdded(albums: feed.recent)
        }
        HomePlayedArtists(artists: feed.artists)
    }
}

private struct HomePlaylists: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    let hiddenWithNone: Bool
    @State private var naming = false

    var body: some View {
        let features = library.features
        let playlists = features.playlists
        if !playlists.isEmpty || !hiddenWithNone {
            Section {
                if playlists.isEmpty {
                    Button {
                        naming = true
                    } label: {
                        ToolRow(title: said("playlists.new"), detail: said("library.playlists.invitation"), symbol: "plus")
                    }
                    .alert("playlists.create", isPresented: $naming) {
                        PlaylistNameField(confirm: said("common.create")) { features.createPlaylist($0) }
                    }
                } else {
                    PlaylistShelf(playlists: Array(library.playlistOrder.sorted(playlists).prefix(HomeFeed.shelved)))
                }
            } header: {
                SeeAllHeader(title: said("home.playlists"), more: !playlists.isEmpty) {
                    navigation.showLibrary(.playlists, in: library)
                }
            }
        }
    }
}

private struct HomeDevices: View {
    @Environment(Library.self) private var library

    var body: some View {
        let features = library.features
        let devices = features.syncSummary?.devices.filter { !$0.thisDevice && ($0.online || $0.nearby) } ?? []
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
