import AppIntents
import ResonateLogic
import ResonateNative
import SwiftUI
import UniformTypeIdentifiers

@main
struct ResonateApp: App {
    @State private var library: Library
    @Environment(\.scenePhase) private var phase

    init() {
        BackdropPipeline.prepare()
        BackgroundHold.register()
        let library = Library()
        _library = State(initialValue: library)
        AppDependencyManager.shared.add(dependency: library)
    }

    var body: some Scene {
        WindowGroup {
            RootView()
                .environment(library)
                .onAppear { library.start() }
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

enum Root: String, Hashable, Codable {
    case library, settings, search, server
}

enum Destination: Hashable, Codable {
    case album(String)
    case artist(String)
    case playlist(UInt64)
    case playlists
    case albums
    case artists
    case favourites
    case tools
    case folders
    case suggestions
    case mix(String)
    case statistics
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
    case background
    case identicalSongs
    case songs
    case served(ServedPlace)

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
    var zones = false
    var filters = false
    var listening = false
    var tagging: TagEditing?
    var deleting: SongsToDelete?
    @ObservationIgnored private var taggingAfterPlayer: TagEditing?
    var songsGrouping = Grouping.songs

    func path(_ root: Root) -> Binding<[Destination]> {
        Binding(get: { self.paths[root] ?? [] }, set: { self.paths[root] = $0 })
    }

    func editTags(_ tracks: [Track]) {
        let editing = TagEditing(tracks: tracks)
        guard nowPlaying else {
            tagging = editing
            return
        }
        taggingAfterPlayer = editing
        nowPlaying = false
    }

    func playerClosed() {
        guard let waiting = taggingAfterPlayer else { return }
        taggingAfterPlayer = nil
        tagging = waiting
    }

    func openSearch() {
        nowPlaying = false
        tab = .search
    }

    func open(_ destination: Destination) {
        nowPlaying = false
        let root = tab == .settings && !destination.belongsToSettings ? Root.library : tab
        tab = root
        guard paths[root]?.last != destination else { return }
        paths[root, default: []].append(destination)
    }

    var coversAmbient: Bool {
        nowPlaying || tab == .search || (paths[tab]?.last?.drawsItsOwnBackdrop ?? false)
    }

    func showSongs() {
        nowPlaying = false
        songsGrouping = .songs
        tab = .library
        paths[.library] = [.songs]
    }

    func leave(_ destination: Destination) {
        for (root, path) in paths where path.last == destination {
            paths[root]?.removeLast()
        }
    }

    var kept: KeptNavigation {
        KeptNavigation(tab: tab, paths: paths.filter { !$0.value.isEmpty })
    }

    func restore(_ kept: KeptNavigation) {
        tab = kept.tab == .server ? .library : kept.tab
        paths = kept.paths.filter { $0.key != .server }
    }

    func leaveServer() {
        guard tab == .server || paths[.server] != nil else { return }
        if tab == .server { tab = .library }
        paths[.server] = nil
    }

    func drop(_ gone: Set<Destination>) {
        guard !gone.isEmpty else { return }
        for (root, path) in paths where path.contains(where: gone.contains) {
            paths[root] = Array(path.prefix { !gone.contains($0) })
        }
    }
}

struct KeptNavigation: Codable, Equatable {
    let tab: Root
    let paths: [Root: [Destination]]

    var destinations: [Destination] {
        Array(Set(paths.values.joined()))
    }

    var encoded: Data? {
        try? JSONEncoder().encode(self)
    }

    static func decoded(_ data: Data) -> KeptNavigation? {
        try? JSONDecoder().decode(KeptNavigation.self, from: data)
    }
}

struct NavigationKeeper: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    @SceneStorage("navigation") private var stored: Data?
    @State private var restored = false

    var body: some View {
        Color.clear
            .accessibilityHidden(true)
            .onAppear(perform: restore)
            .onChange(of: navigation.kept) { _, now in
                guard restored else { return }
                stored = now.encoded
            }
    }

    private func restore() {
        guard !restored else { return }
        restored = true
        guard let stored, let kept = KeptNavigation.decoded(stored) else { return }
        navigation.restore(kept)
        let destinations = kept.destinations
        guard !destinations.isEmpty else { return }
        Task {
            let gone = await library.gone(among: destinations)
            navigation.drop(gone)
        }
    }
}

extension Destination {
    var drawsItsOwnBackdrop: Bool {
        switch self {
        case .album, .artist, .playlist, .mix, .songLyrics, .visualizer: true
        default: false
        }
    }

    var belongsToSettings: Bool {
        switch self {
        case .sound, .processing, .output, .scrobbling, .providers, .sync, .folders, .appearance, .background: true
        default: false
        }
    }
}

struct ErasingOverlay: View {
    @Environment(Library.self) private var library

    var body: some View {
        if library.erasing {
            ZStack {
                Rectangle()
                    .fill(.black.opacity(0.4))
                    .ignoresSafeArea()
                    .contentShape(Rectangle())
                    .onTapGesture {}
                Group {
                    if let left = library.erasedLeaving {
                        Text((["Erasing did not finish."] + left.sentences + ["Close Resonate, open it again and erase once more."]).joined(separator: " "))
                    } else {
                        ProgressView("Erasing…")
                    }
                }
                .multilineTextAlignment(.center)
                .padding(24)
                .glassEffect(.regular, in: .rect(cornerRadius: 20))
                .padding(.horizontal, 24)
            }
            .accessibilityElement(children: .combine)
            .accessibilityAddTraits(.isModal)
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
            Tab("Library", systemImage: "music.note.house", value: Root.library) {
                stack(.library) { LibraryView() }
            }
            if library.features.serving {
                Tab("Server", systemImage: "server.rack", value: Root.server) {
                    stack(.server) { ServerView() }
                }
            }
            Tab("Settings", systemImage: "gearshape", value: Root.settings) {
                stack(.settings) { SettingsView() }
            }
            Tab(value: Root.search, role: .search) {
                stack(.search) { SearchTab() }
            }
        }
        .tabBarMinimizeBehavior(.onScrollDown)
        .onChange(of: library.features.serving) { _, serving in
            if !serving { navigation.leaveServer() }
        }
        .background { NavigationKeeper() }
        .task { await PlayerWarmup.warm() }
        .tint(AppearanceState.shared.tint)
        .modifier(PlayerAccessory(loaded: library.loaded, zoom: player))
        .safeAreaInset(edge: .top) { StatusBanner() }
        .fullScreenCover(isPresented: $navigation.nowPlaying, onDismiss: navigation.playerClosed) {
            NowPlayingView()
                .tint(AppearanceState.shared.tint)
                .navigationTransition(.zoom(sourceID: PlayerTransition.source, in: player))
                .environment(navigation)
        }
        .modifier(DeleteSongsConfirmation(insidePlayer: false))
        .modifier(SleepTimerPresentation(presenting: !navigation.nowPlaying))
        .modifier(ZonesPresentation(presenting: !navigation.nowPlaying))
        .sheet(isPresented: $navigation.filters) {
            FilterSheet()
        }
        .sheet(item: $navigation.tagging) { editing in
            TagEditorSheet(tracks: editing.tracks)
        }
        .modifier(SharingSheet(presenting: !navigation.nowPlaying))
        .modifier(PairingOffer(presenting: !navigation.nowPlaying))
        .onChange(of: library.features.playlistToShow) { _, shown in
            guard let shown else { return }
            library.features.playlistToShow = nil
            navigation.open(.playlist(shown))
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
        .disabled(library.erasing)
        .overlay { ErasingOverlay() }
        .onChange(of: library.erasing) { _, erasing in
            guard erasing else { return }
            navigation.nowPlaying = false
            navigation.sleepTimer = false
            navigation.zones = false
            navigation.filters = false
            navigation.listening = false
            navigation.choosingFolder = false
            navigation.tagging = nil
            library.features.sharing = nil
        }
        .environment(navigation)
    }

    private func stack<Content: View>(_ root: Root, @ViewBuilder content: () -> Content) -> some View {
        NavigationStack(path: navigation.path(root)) {
            content()
                .modifier(ThemedPage())
                .navigationDestination(for: Destination.self) { DestinationView(destination: $0).modifier(ThemedPage()) }
        }
    }
}

struct PairingOffer: ViewModifier {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    let presenting: Bool

    func body(content: Content) -> some View {
        content.alert(
            "Join these devices?",
            isPresented: Binding(
                get: { presenting && library.features.pairingOffered != nil },
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
        } message: { code in
            Text(library.features.syncConfig?.key.isEmpty == false
                ? "A link asks this iPhone to leave the devices it syncs with now and sync with the devices of the code \(code). Join only if that is your code or one you were given."
                : "A link asks this iPhone to share its playlists, favourites and history with the devices of the code \(code). Join only if that is your code or one you were given.")
        }
    }
}

extension View {
    @MainActor
    func themedRows() -> some View {
        listRowBackground(AppearanceState.shared.themedBackground ? Color.clear : nil)
    }

    @MainActor
    func themedCards() -> some View {
        listRowBackground(AppearanceState.shared.themedBackground ? Color.themedCard : nil)
    }
}

struct ThemedList<Content: View>: View {
    @ViewBuilder let content: Content

    var body: some View {
        List { content.themedCards() }
    }
}

struct ThemedForm<Content: View>: View {
    @ViewBuilder let content: Content

    var body: some View {
        Form { content.themedCards() }
    }
}

struct ThemedPage: ViewModifier {
    @Environment(Navigation.self) private var navigation

    func body(content: Content) -> some View {
        let appearance = AppearanceState.shared
        if appearance.themedBackground {
            content
                .scrollContentBackground(.hidden)
                .containerBackground(for: .navigation) {
                    AppBackdrop(
                        strength: Float(appearance.backgroundStrength),
                        moving: appearance.movingBackground,
                        frozen: navigation.coversAmbient
                    )
                }
        } else {
            content
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
        case .playlists: LibraryView(page: .playlists)
        case .albums: LibraryView(page: .albums)
        case .artists: LibraryView(page: .artists)
        case .favourites: FavouritesView()
        case .tools: LibraryToolsView()
        case .folders: FoldersView()
        case .suggestions: SuggestionsView()
        case .mix(let id): MixView(id: id)
        case .statistics: StatisticsView()
        case .wanted: WantedView()
        case .providers: ProviderView()
        case .scrobbling: ScrobblingView()
        case .sync: SyncView()
        case .output: OutputView()
        case .sound: SoundSettingsView()
        case .processing: ProcessingView()
        case .appearance: AppearanceView()
        case .background: BackgroundView()
        case .identicalSongs: IdenticalSongsView()
        case .songs: SongsView()
        case .served(let place): ServedPlaceView(place: place)
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
    private static let fade: Animation = .linear(duration: 0.12)

    @Environment(Library.self) private var library
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        let arrival: AnyTransition = reduceMotion ? .opacity : .move(edge: .top).combined(with: .opacity)
        VStack(spacing: 8) {
            if library.scanShown {
                ScanProgress()
                    .transition(arrival)
            }
            if let activity = library.features.syncActivity {
                SyncProgress(activity: activity)
                    .transition(arrival)
            }
            if let message = library.status {
                StatusCard(message: message)
                    .id(message.serial)
                    .transition(arrival)
            }
        }
        .motion(.smooth, calm: Self.fade, value: library.status)
        .motion(.smooth, calm: Self.fade, value: library.scanShown)
        .motion(.smooth, calm: Self.fade, value: library.features.syncActivity == nil)
    }
}

struct SyncProgress: View {
    @Environment(Navigation.self) private var navigation
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    let activity: SyncActivity

    var body: some View {
        Button {
            navigation.tab = .settings
            if navigation.paths[.settings]?.last != .sync {
                navigation.paths[.settings, default: []].append(.sync)
            }
        } label: {
            HStack(spacing: 12) {
                Group {
                    switch activity {
                    case .fetching(_, let fraction?):
                        ProgressView(value: fraction)
                            .progressViewStyle(.circular)
                    case .fetching, .syncing:
                        ProgressView()
                    }
                }
                .frame(width: BannerGlyph.edge, height: BannerGlyph.edge)
                VStack(alignment: .leading, spacing: 1) {
                    Text(title)
                        .font(.subheadline.weight(.semibold))
                    if let detail {
                        Text(detail)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                            .monospacedDigit()
                            .contentTransition(reduceMotion ? .identity : .numericText())
                    }
                }
                Spacer(minLength: 0)
                Image(systemName: "chevron.right")
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(.tertiary)
            }
            .frame(minHeight: BannerGlyph.row)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .bannerGlass()
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
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        HStack(spacing: 12) {
            ProgressView()
                .frame(width: BannerGlyph.edge, height: BannerGlyph.edge)
            Text("Reading music folder… \(counted(library.scanned, "song"))")
                .font(.subheadline)
                .monospacedDigit()
                .contentTransition(reduceMotion ? .identity : .numericText())
            Spacer(minLength: 0)
            Button("Stop") { library.stopScanning() }
                .font(.footnote.weight(.semibold))
                .buttonStyle(.bordered)
                .buttonBorderShape(.capsule)
                .controlSize(.small)
        }
        .frame(minHeight: BannerGlyph.row)
        .bannerGlass()
        .accessibilityElement(children: .combine)
    }
}

extension StatusMessage.Offer {
    var title: String {
        switch self {
        case .openCatalog: "Try again"
        case .restoreQueueRow, .restoreEqualiser: "Undo"
        }
    }
}

struct BannerGlyph: View {
    static let edge: CGFloat = 28
    static let row: CGFloat = 44

    let symbol: String
    let colour: Color

    var body: some View {
        Image(systemName: symbol)
            .font(.title3.weight(.semibold))
            .foregroundStyle(colour)
            .frame(width: Self.edge, height: Self.edge)
            .accessibilityHidden(true)
    }
}

extension View {
    func bannerGlass(tint: Color? = nil) -> some View {
        let glass = tint.map { Glass.regular.tint($0) } ?? Glass.regular
        return padding(.horizontal, 14)
            .padding(.vertical, 4)
            .glassEffect(glass, in: .rect(cornerRadius: 22))
            .padding(.horizontal, 12)
    }
}

struct StatusCard: View {
    private static let failureTint = Color.orange.opacity(0.2)

    @Environment(Library.self) private var library
    @Environment(\.dynamicTypeSize) private var textSize
    let message: StatusMessage
    @State private var expanded = false

    var body: some View {
        let failed = message.kind == .failure

        HStack(spacing: 12) {
            BannerGlyph(
                symbol: failed ? "exclamationmark.triangle.fill" : "info.circle.fill",
                colour: failed ? .orange : Color.worn
            )
            Text(message.text)
                .font(.subheadline)
                .lineLimit(expanded || textSize.isAccessibilitySize ? nil : 3)
                .frame(maxWidth: .infinity, minHeight: BannerGlyph.row, alignment: .leading)
                .contentShape(Rectangle())
                .onTapGesture { expanded.toggle() }
            if let offer = message.offer {
                Button(offer.title) { library.perform(offer) }
                    .font(.footnote.weight(.semibold))
                    .buttonStyle(.bordered)
                    .buttonBorderShape(.capsule)
                    .controlSize(.small)
            }
            Button {
                library.dismissStatus()
            } label: {
                Image(systemName: "xmark")
                    .font(.footnote.weight(.bold))
                    .foregroundStyle(.secondary)
                    .touchTarget()
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Dismiss")
        }
        .bannerGlass(tint: failed ? Self.failureTint : nil)
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
