import ResonateNative
import SwiftUI
import UniformTypeIdentifiers

@main
struct ResonateApp: App {
    @State private var library = Library()
    @Environment(\.scenePhase) private var phase

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
            if now != .active { library.save() }
        }
    }
}

enum Root: Hashable {
    case songs, library, queue, settings
}

enum Destination: Hashable {
    case album(title: String, artist: String)
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
    case autoEq
    case output
    case sound
    case processing
    case lyrics(Track?)
    case visualizer
    case analysis(Track?)
    case inspector(Track)
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

    func path(_ root: Root) -> Binding<[Destination]> {
        Binding(get: { self.paths[root] ?? [] }, set: { self.paths[root] = $0 })
    }

    func open(_ destination: Destination) {
        nowPlaying = false
        let root = tab == .settings && !destination.belongsToSettings ? Root.library : tab
        tab = root
        paths[root, default: []].append(destination)
    }
}

extension Destination {
    var belongsToSettings: Bool {
        switch self {
        case .sound, .processing, .output, .autoEq, .scrobbling, .providers, .sync, .folders: true
        default: false
        }
    }
}

struct RootView: View {
    @Environment(Library.self) private var library
    @State private var navigation = Navigation()

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
        }
        .tabBarMinimizeBehavior(.onScrollDown)
        .tabViewBottomAccessory {
            if library.snapshot?.current != nil {
                MiniPlayer()
            }
        }
        .safeAreaInset(edge: .top) { StatusBanner() }
        .fullScreenCover(isPresented: $navigation.nowPlaying) {
            NowPlayingView()
                .environment(navigation)
        }
        .sheet(isPresented: $navigation.sleepTimer) {
            SleepTimerSheet().presentationDetents([.medium])
        }
        .sheet(isPresented: $navigation.filters) {
            FilterSheet()
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

struct DestinationView: View {
    let destination: Destination

    var body: some View {
        switch destination {
        case .album(let title, let artist): AlbumView(title: title, artist: artist)
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
        case .autoEq: AutoEqView()
        case .output: OutputView()
        case .sound: SoundSettingsView()
        case .processing: ProcessingView()
        case .lyrics(let track): LyricsView(chosen: track)
        case .visualizer: VisualizerView()
        case .analysis(let track): AnalysisView(chosen: track)
        case .inspector(let track): InspectorView(track: track)
        }
    }
}

struct StatusBanner: View {
    @Environment(Library.self) private var library

    var body: some View {
        VStack(spacing: 0) {
            if library.scanning {
                ProgressView().progressViewStyle(.linear)
            }
            if let message = library.status {
                HStack {
                    Text(message).font(.footnote).lineLimit(3)
                    Spacer()
                    Button("Dismiss") { library.status = nil }
                        .font(.footnote.weight(.semibold))
                }
                .padding(.horizontal, 16)
                .padding(.vertical, 8)
                .glassEffect(.regular, in: .rect(cornerRadius: 16))
                .padding(.horizontal, 12)
                .transition(.move(edge: .top).combined(with: .opacity))
            }
        }
        .animation(.smooth, value: library.status)
    }
}

struct LibraryToolbar: ToolbarContent {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation

    var body: some ToolbarContent {
        ToolbarItemGroup(placement: .topBarTrailing) {
            if library.snapshot?.current != nil {
                Button("Sleep timer", systemImage: library.snapshot?.sleep == nil ? "moon" : "moon.fill") {
                    navigation.sleepTimer = true
                }
            }
            Button("Add music folder", systemImage: "folder.badge.plus") {
                navigation.choosingFolder = true
            }
            .disabled(library.scanning)
            Button("Library tools", systemImage: "ellipsis") {
                navigation.open(.tools)
            }
        }
    }
}
