import ResonateNative
import SwiftUI
import UniformTypeIdentifiers

@main
struct ResonateApp: App {
    @State private var library = Library()
    @Environment(\.scenePhase) private var phase

    init() {
        BackgroundHold.register()
    }

    var body: some Scene {
        WindowGroup {
            RootView()
                .environment(library)
                .onAppear {
                    BackdropPipeline.prepare()
                    library.start()
                }
                .onOpenURL { library.open($0) }
        }
        .onChange(of: phase) { _, now in
            library.features.foreground = now == .active
            switch now {
            case .active: library.features.cameForward()
            case .background:
                library.features.wentToBackground()
                library.save()
            default: library.save()
            }
        }
    }
}

enum Root: Hashable {
    case songs, library, queue, settings, search
}

enum Destination: Hashable {
    case album(String)
    case artist(String)
    case playlist(UInt64)
    case playlists
    case favourites
    case tools
    case folders
    case suggestions
    case statistics
    case online
    case wanted
    case providers
    case scrobbling
    case sync
    case output
    case sound
    case processing
    case songLyrics(UInt64?)
    case visualizer
    case songAnalysis(UInt64?)
    case songDetails(UInt64)
    case appearance

    static func lyrics(_ track: Track?) -> Destination {
        .songLyrics(track?.id)
    }

    static func analysis(_ track: Track?) -> Destination {
        .songAnalysis(track?.id)
    }

    static func inspector(_ track: Track) -> Destination {
        .songDetails(track.id)
    }
}

enum PlayerTransition {
    static let source = "player"
}

@MainActor
@Observable
final class Navigation {
    var tab: Root = .library
    var paths: [Root: [Destination]] = [:]
    var nowPlaying = false
    var choosingFolder = false
    var sleepTimer = false
    var filters = false
    var listening = false
    var tagging: TagEditing?
    var songsGrouping = Grouping.songs
    var searchPlace = SearchPlace.library

    func path(_ root: Root) -> Binding<[Destination]> {
        Binding(get: { self.paths[root] ?? [] }, set: { self.paths[root] = $0 })
    }

    func searchOnline() {
        nowPlaying = false
        searchPlace = .online
        tab = .search
    }

    func open(_ destination: Destination) {
        guard destination != .online else {
            searchOnline()
            return
        }

        nowPlaying = false
        let root = tab == .settings && !destination.belongsToSettings ? Root.library : tab
        tab = root
        paths[root, default: []].append(destination)
    }

    func leave(_ destination: Destination) {
        for (root, path) in paths where path.last == destination {
            paths[root]?.removeLast()
        }
    }
}

extension Destination {
    var belongsToSettings: Bool {
        switch self {
        case .sound, .processing, .output, .scrobbling, .providers, .sync, .folders, .appearance: true
        default: false
        }
    }
}

struct RootView: View {
    @Environment(Library.self) private var library
    @State private var navigation = Navigation()
    @Namespace private var player

    var body: some View {
        @Bindable var navigation = navigation
        TabView(selection: $navigation.tab) {
            Tab("Songs", systemImage: "music.note", value: Root.songs) {
                stack(.songs) { SongsView() }
            }
            Tab("Library", systemImage: "music.note.house", value: Root.library) {
                stack(.library) { LibraryView() }
            }
            Tab("Queue", systemImage: "list.bullet", value: Root.queue) {
                stack(.queue) { QueueView() }
            }
            Tab("Settings", systemImage: "gearshape", value: Root.settings) {
                stack(.settings) { SettingsView() }
            }
            Tab(value: Root.search, role: .search) {
                stack(.search) { SearchTab() }
            }
        }
        .tabBarMinimizeBehavior(.onScrollDown)
        .tint(AppearanceState.shared.tint)
        .modifier(PlayerAccessory(loaded: library.loaded, zoom: player))
        .safeAreaInset(edge: .top) { StatusBanner() }
        .fullScreenCover(isPresented: $navigation.nowPlaying) {
            NowPlayingView()
                .tint(AppearanceState.shared.tint)
                .navigationTransition(.zoom(sourceID: PlayerTransition.source, in: player))
                .environment(navigation)
        }
        .sheet(isPresented: $navigation.sleepTimer) {
            SleepTimerSheet().presentationDetents([.medium, .large])
        }
        .sheet(isPresented: $navigation.filters) {
            FilterSheet()
        }
        .sheet(item: $navigation.tagging) { editing in
            TagEditorSheet(tracks: editing.tracks)
        }
        .alert(
            "Join these devices?",
            isPresented: Binding(
                get: { library.features.pairingOffered != nil },
                set: { shown in if !shown { library.features.pairingOffered = nil } }
            ),
            presenting: library.features.pairingOffered
        ) { code in
            Button("Join") {
                library.features.joinSync(code)
                library.features.pairingOffered = nil
                navigation.nowPlaying = false
                navigation.tab = .settings
                navigation.paths[.settings] = [.sync]
            }
            Button("Cancel", role: .cancel) {}
        } message: { _ in
            Text(library.features.syncConfig?.key.isEmpty == false
                ? "This iPhone leaves the devices it syncs with now and syncs with the devices of the code you scanned."
                : "This iPhone syncs its playlists, favourites and history with the devices of the code you scanned.")
        }
        .sheet(isPresented: $navigation.listening) {
            ListenSheet()
                .presentationDetents([.medium, .large])
                .environment(navigation)
        }
        .fileImporter(isPresented: $navigation.choosingFolder, allowedContentTypes: [.folder]) { result in
            switch result {
            case .success(let url): library.addFolder(url)
            case .failure(let failure): library.report(failure)
            }
        }
        .environment(navigation)
    }

    private func stack<Content: View>(_ root: Root, @ViewBuilder content: () -> Content) -> some View {
        NavigationStack(path: navigation.path(root)) {
            content()
                .navigationDestination(for: Destination.self) { DestinationView(destination: $0) }
        }
    }
}

struct PlayerAccessory: ViewModifier {
    let loaded: Bool
    let zoom: Namespace.ID

    func body(content: Content) -> some View {
        if #available(iOS 26.1, *) {
            content.tabViewBottomAccessory(isEnabled: loaded) {
                MiniPlayer(zoom: zoom)
            }
        } else {
            content.tabViewBottomAccessory {
                if loaded { MiniPlayer(zoom: zoom) }
            }
        }
    }
}

struct DestinationView: View {
    let destination: Destination

    var body: some View {
        switch destination {
        case .album(let key): AlbumView(key: key)
        case .artist(let name): ArtistView(name: name)
        case .playlist(let id): PlaylistView(id: id)
        case .playlists: LibraryView(playlistsOnly: true)
        case .favourites: FavouritesView()
        case .tools: LibraryToolsView()
        case .folders: FoldersView()
        case .suggestions: SuggestionsView()
        case .statistics: StatisticsView()
        case .online: OnlineView()
        case .wanted: WantedView()
        case .providers: ProviderView()
        case .scrobbling: ScrobblingView()
        case .sync: SyncView()
        case .output: OutputView()
        case .sound: SoundSettingsView()
        case .processing: ProcessingView()
        case .appearance: AppearanceView()
        case .songLyrics(let id): LiveSong(id: id) { LyricsView(chosen: $0) }
        case .visualizer: VisualizerView()
        case .songAnalysis(let id): LiveSong(id: id) { AnalysisView(chosen: $0) }
        case .songDetails(let id): LiveSong(id: id) { track in track.map { InspectorView(track: $0) } }
        }
    }
}

struct LiveSong<Content: View>: View {
    @Environment(Library.self) private var library
    let id: UInt64?
    @ViewBuilder let content: (Track?) -> Content

    var body: some View {
        if let id {
            if let track = library.track(id) {
                content(track)
            } else {
                ContentUnavailableView(
                    "This song is no longer here",
                    systemImage: "music.note",
                    description: Text("It may have been removed from your library.")
                )
            }
        } else {
            content(nil)
        }
    }
}

struct StatusBanner: View {
    @Environment(Library.self) private var library

    var body: some View {
        VStack(spacing: 8) {
            if library.scanning {
                ScanProgress()
                    .transition(.move(edge: .top).combined(with: .opacity))
            }
            if let activity = library.features.syncActivity {
                SyncProgress(activity: activity)
                    .transition(.move(edge: .top).combined(with: .opacity))
            }
            if let message = library.status {
                StatusCard(message: message)
                    .id(message.serial)
                    .transition(.move(edge: .top).combined(with: .opacity))
            }
        }
        .animation(.smooth, value: library.status)
        .animation(.smooth, value: library.scanning)
        .animation(.smooth, value: library.features.syncActivity == nil)
    }
}

struct SyncProgress: View {
    @Environment(Navigation.self) private var navigation
    let activity: SyncActivity

    var body: some View {
        Button {
            navigation.tab = .settings
            if navigation.paths[.settings]?.last != .sync {
                navigation.paths[.settings, default: []].append(.sync)
            }
        } label: {
            HStack(spacing: 12) {
                switch activity {
                case .fetching(_, let fraction?):
                    ProgressView(value: fraction)
                        .progressViewStyle(.circular)
                case .fetching, .syncing:
                    ProgressView()
                }
                VStack(alignment: .leading, spacing: 1) {
                    Text(title)
                        .font(.footnote.weight(.semibold))
                    if let detail {
                        Text(detail)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                            .monospacedDigit()
                            .contentTransition(.numericText())
                    }
                }
                Spacer(minLength: 0)
                Image(systemName: "chevron.right")
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(.tertiary)
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .padding(.horizontal, 16)
        .padding(.vertical, 8)
        .glassEffect(.regular, in: .rect(cornerRadius: 16))
        .padding(.horizontal, 12)
        .accessibilityElement(children: .combine)
        .accessibilityHint("Opens device sync")
    }

    private var title: String {
        switch activity {
        case .syncing: "Syncing with your devices…"
        case .fetching(let songs, _): "Fetching \(counted(songs, "song")) from your devices"
        }
    }

    private var detail: String? {
        guard case .fetching(_, let fraction?) = activity else { return nil }
        return "\(Int((fraction * 100).rounded()))%"
    }
}

struct ScanProgress: View {
    @Environment(Library.self) private var library

    var body: some View {
        HStack(spacing: 12) {
            ProgressView()
            Text("Reading music folder… \(counted(library.scanned, "song"))")
                .font(.footnote)
                .monospacedDigit()
                .contentTransition(.numericText())
            Spacer(minLength: 0)
            Button("Stop") { library.stopScanning() }
                .font(.footnote.weight(.semibold))
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 8)
        .glassEffect(.regular, in: .rect(cornerRadius: 16))
        .padding(.horizontal, 12)
        .accessibilityElement(children: .combine)
    }
}

struct StatusCard: View {
    @Environment(Library.self) private var library
    let message: StatusMessage

    var body: some View {
        HStack(spacing: 10) {
            if message.kind == .failure {
                Image(systemName: "exclamationmark.triangle.fill")
                    .foregroundStyle(.orange)
                    .accessibilityHidden(true)
            }
            Text(message.text).font(.footnote).lineLimit(3)
            Spacer(minLength: 0)
            Button("Dismiss") { library.dismissStatus() }
                .font(.footnote.weight(.semibold))
                .buttonStyle(.borderless)
                .padding(.vertical, 6)
                .contentShape(Rectangle())
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 2)
        .glassEffect(.regular, in: .rect(cornerRadius: 16))
        .padding(.horizontal, 12)
    }
}

struct LibraryToolbar: ToolbarContent {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation

    var body: some ToolbarContent {
        ToolbarItem(placement: .topBarTrailing) {
            Menu("More", systemImage: "ellipsis") {
                Button("Add music folder", systemImage: "folder.badge.plus") {
                    navigation.choosingFolder = true
                }
                .disabled(library.scanning)
                if library.scanning {
                    Button("Stop reading", systemImage: "stop.circle") { library.stopScanning() }
                } else {
                    Button("Rescan library", systemImage: "arrow.clockwise") { library.rescan() }
                }
                Button("Library tools", systemImage: "wrench.and.screwdriver") {
                    navigation.open(.tools)
                }
            }
        }
    }
}
