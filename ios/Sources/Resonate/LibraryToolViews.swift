import ResonateLogic
import ResonateNative
import SwiftUI

struct ToolRow: View {
    @ScaledMetric(relativeTo: .body) private var iconColumn: CGFloat = 28
    let title: String
    let detail: String
    let symbol: String
    var ink: Color?
    var detailLines = 2

    var body: some View {
        HStack(spacing: 14) {
            Image(systemName: symbol)
                .font(.body.weight(.medium))
                .foregroundStyle(ink.map(AnyShapeStyle.init) ?? AnyShapeStyle(.tint))
                .frame(width: iconColumn)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 2) {
                Text(title)
                    .foregroundStyle(ink ?? Color.primary)
                    .titleLines(2)
                if !detail.isEmpty {
                    Text(detail)
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                        .titleLines(detailLines)
                }
            }
        }
        .accessibilityElement(children: .combine)
    }
}

struct LibraryToolsView: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation

    var body: some View {
        let current = library.playingTrack
        ThemedList {
            Section("library.title") {
                NavigationLink(value: Destination.playlists) {
                    ToolRow(title: said("library.playlists"), detail: said("tools.playlists.detail"), symbol: "music.note.list")
                }
                NavigationLink(value: Destination.suggestions) {
                    ToolRow(title: said("tools.suggestions"), detail: said("tools.suggestions.detail"), symbol: "square.stack")
                }
                NavigationLink(value: Destination.statistics) {
                    ToolRow(title: said("tools.statistics"), detail: said("tools.statistics.detail"), symbol: "chart.bar")
                }
                NavigationLink(value: Destination.party) {
                    ToolRow(title: said("tools.party"), detail: said("tools.party.detail"), symbol: "person.2.wave.2")
                }
                NavigationLink(value: Destination.identicalSongs) {
                    ToolRow(title: said("tools.identical"), detail: said("tools.identical.detail"), symbol: "square.on.square")
                }
                Button { navigation.filters = true } label: {
                    ToolRow(title: said("filter.title"), detail: said("tools.filter.detail"), symbol: "line.3.horizontal.decrease.circle")
                }
            }
            Section("tools.discover") {
                Button { navigation.openSearch() } label: {
                    ToolRow(title: said("tools.search"), detail: said("tools.search.detail"), symbol: "magnifyingglass")
                }
                Button { navigation.listening = true } label: {
                    ToolRow(title: said("tools.listen"), detail: said("tools.listen.detail"), symbol: "ear")
                }
                NavigationLink(value: Destination.wanted) {
                    ToolRow(title: said("tools.saved-for-later"), detail: said("tools.saved-for-later.detail"), symbol: "bookmark")
                }
                RefreshArtworkRow()
                if library.features.organiseUndo != nil {
                    Button { library.features.putFilesBack() } label: {
                        ToolRow(title: said("tools.put-files-back"), detail: said("tools.put-files-back.detail"), symbol: "arrow.uturn.backward.circle")
                    }
                    .disabled(library.features.organising)
                }
                if library.features.tagsUndoable {
                    Button { library.features.undoTags() } label: {
                        ToolRow(title: said("tools.undo-tags"), detail: said("tools.undo-tags.detail"), symbol: "arrow.uturn.backward")
                    }
                }
            }
            Section {
                NavigationLink(value: current.map { _ in Destination.lyrics(nil) }) {
                    ToolRow(title: said("tools.lyrics"), detail: said("tools.lyrics.detail"), symbol: "quote.bubble")
                }
                NavigationLink(value: current.map { _ in Destination.visualizer }) {
                    ToolRow(title: said("tools.visualizer"), detail: said("tools.visualizer.detail"), symbol: "waveform")
                }
                NavigationLink(value: current.map { _ in Destination.analysis(nil) }) {
                    ToolRow(title: said("tools.analysis"), detail: said("tools.analysis.detail"), symbol: "waveform.path.ecg")
                }
                NavigationLink(value: current.map(Destination.inspector)) {
                    ToolRow(title: said("tools.inspector"), detail: said("tools.inspector.detail"), symbol: "info.circle")
                }
            } header: {
                Text("tools.current-song")
            } footer: {
                if current == nil { Text("tools.current-song.none") }
            }
            .disabled(current == nil)
            Section("tools.preferences") {
                Button { navigation.tab = .settings } label: {
                    ToolRow(title: said("tools.settings"), detail: said("tools.settings.detail"), symbol: "gearshape")
                }
            }
        }
        .buttonStyle(.plain)
        .navigationTitle("tools.title")
    }
}

struct FoldersView: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    @State private var removing: MusicFolder?
    @State private var organising: MusicFolder?

    var body: some View {
        ThemedForm {
            Section {
                ForEach(library.folderList) { folder in
                    FolderRow(
                        folder: folder,
                        scanning: library.scanning,
                        rescan: { library.scan(folder.id) },
                        organise: { organising = folder },
                        remove: { removing = folder }
                    )
                    .swipeActions {
                        if folder.id != Folders.local {
                            Button("folders.remove-access", role: .destructive) { removing = folder }
                                .tint(.red)
                        }
                    }
                }
            } footer: {
                Text("folders.footer")
            }
            Section {
                Button { navigation.choosingFolder = true } label: {
                    ToolRow(title: said("folders.add"), detail: said("folders.add.detail"), symbol: "folder.badge.plus")
                }
                RescanRow()
            }
        }
        .navigationTitle("folders.title")
        .sheet(item: $organising) { OrganiseSheet(folder: $0).sheetSized(.long) }
        .task { library.folderList = await library.folders.refreshed() }
        .confirmationDialog(
            said("folders.remove.title", removing?.name ?? said("folders.remove.this-folder")),
            isPresented: Binding(get: { removing != nil }, set: { if !$0 { removing = nil } }),
            titleVisibility: .visible,
            presenting: removing
        ) { folder in
            Button("folders.remove-access", role: .destructive) { library.removeFolder(folder) }
        } message: { _ in
            Text("folders.remove.message")
        }
    }
}

private struct RescanRow: View {
    @Environment(Library.self) private var library

    var body: some View {
        let readable = library.folderList.contains(where: \.readable)
        if library.scanning {
            HStack(spacing: 14) {
                ToolRow(title: said("folders.scanning"), detail: said("folders.scanning.detail", Int(library.scanned)), symbol: "arrow.clockwise")
                Spacer(minLength: 0)
                Button("folders.scanning.stop") { library.stopScanning() }
                    .buttonStyle(.bordered)
                    .buttonBorderShape(.capsule)
            }
        } else {
            Button { library.rescan() } label: {
                ToolRow(
                    title: said("folders.rescan-library"),
                    detail: readable ? said("folders.rescan-library.detail") : said("folders.rescan-library.no-folder"),
                    symbol: "arrow.clockwise"
                )
            }
            .disabled(!readable)
        }
    }
}

struct FolderRow: View {
    let folder: MusicFolder
    let scanning: Bool
    let rescan: () -> Void
    let organise: () -> Void
    let remove: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(alignment: .firstTextBaseline, spacing: 12) {
                Image(systemName: folder.id == Folders.local ? "iphone" : "folder")
                    .foregroundStyle(.tint)
                    .accessibilityHidden(true)
                VStack(alignment: .leading, spacing: 2) {
                    Text(folder.name)
                    Text(standing)
                        .font(.caption)
                        .foregroundStyle(folder.readable ? Color.secondary : Color.failure)
                    if !folder.path.isEmpty {
                        Text(folder.path)
                            .font(.caption2.monospaced())
                            .foregroundStyle(.secondary)
                            .lineLimit(2)
                            .truncationMode(.middle)
                            .textSelection(.enabled)
                    }
                }
            }
            ViewThatFits(in: .horizontal) {
                HStack { actions }
                VStack(alignment: .leading) { actions }
            }
            .buttonStyle(.bordered)
            .controlSize(.large)
        }
    }

    @ViewBuilder
    private var actions: some View {
        Button("folders.rescan", systemImage: "arrow.clockwise", action: rescan)
            .disabled(!folder.readable || scanning)
        Button("organise.title", systemImage: "folder.badge.gearshape", action: organise)
            .disabled(!folder.writable || scanning)
        if folder.id != Folders.local {
            Button("folders.remove-access", systemImage: "xmark.circle", role: .destructive, action: remove)
        }
    }

    private var standing: String {
        if !folder.readable { return said("folders.standing.unavailable") }
        return folder.writable ? said("folders.standing.available") : said("folders.standing.read-only")
    }
}

struct SuggestedMix: Identifiable, Sendable {
    enum Source: Sendable {
        case filter(TrackFilter)
        case search(SavedSearch)
    }

    let id: String
    let title: String
    let detail: String
    let shelfName: String
    let source: Source
    let mark: String

    init(list: SuggestedList) {
        id = list.id
        title = list.name
        detail = list.detail
        shelfName = list.shelfName
        source = .search(list.search)
        mark = Suggestions.symbol(for: list.reason)
    }

    init(id: String, title: String, detail: String, filter: TrackFilter, mark: String) {
        self.id = id
        self.title = title
        self.detail = detail
        shelfName = Suggestions.yoursShelf
        source = .filter(filter)
        self.mark = mark
    }

    var kept: SavedSearch? {
        if case .search(let search) = source { search } else { nil }
    }

    static var yours: [SuggestedMix] { madeYours() }

    private static func madeYours() -> [SuggestedMix] {
        var favourites = defaultTrackFilter()
        favourites.favourites = true
        var recent = defaultTrackFilter()
        recent.order = .played
        recent.descending = true
        recent.played = true
        return [
            SuggestedMix(id: "your-favourites", title: said("mix.yours.favourites"), detail: said("mix.yours.favourites.detail"), filter: favourites, mark: "heart.fill"),
            SuggestedMix(id: "recently-played", title: said("mix.yours.recent"), detail: said("mix.yours.recent.detail"), filter: recent, mark: "clock.arrow.circlepath"),
        ]
    }

    nonisolated func tracks(in native: MobilePlayer) throws -> [Track] {
        switch source {
        case .filter(let filter):
            try Library.everyPage { try native.filteredTracks(filter: filter, offset: $0, limit: Library.page) }
        case .search(let search):
            try Library.everyPage { try native.savedSearchPage(search: search, offset: $0, limit: Library.page) }
        }
    }

    nonisolated func covers(in native: MobilePlayer) throws -> [UInt64] {
        switch source {
        case .filter(let filter): try native.picturedBy(filter: filter)
        case .search(let search): try native.measureSavedSearch(search: search).covers
        }
    }
}

struct OfferedMixes: Equatable, Sendable {
    var lists: [SuggestedList] = []
    var covers: [String: [UInt64]] = [:]

    var all: [SuggestedMix] { SuggestedMix.yours + lists.map(SuggestedMix.init(list:)) }

    var home: [SuggestedMix] { SuggestedMix.yours + Suggestions.home(lists).map(SuggestedMix.init(list:)) }

    var shelves: [(name: String, mixes: [SuggestedMix])] {
        var found: [(name: String, mixes: [SuggestedMix])] = []
        for mix in all {
            if let at = found.firstIndex(where: { $0.name == mix.shelfName }) {
                found[at].mixes.append(mix)
            } else {
                found.append((mix.shelfName, [mix]))
            }
        }
        return found
    }

    func named(_ id: String) -> SuggestedMix? { all.first { $0.id == id } }

    @MainActor
    static func read(in library: Library) async -> OfferedMixes? {
        guard let native = try? await library.player() else { return nil }
        let zone = TimeZone.current
        let identifier = zone.identifier
        let offset = Int32(zone.secondsFromGMT())
        return try? await Work.onScreen {
            let lists = try native.suggestedLists(timeZone: identifier, utcOffsetSeconds: offset)
            var covers = Dictionary(uniqueKeysWithValues: lists.map { ($0.id, $0.covers) })
            for mix in SuggestedMix.yours {
                covers[mix.id] = try mix.covers(in: native)
            }
            return OfferedMixes(lists: lists, covers: covers)
        }
    }
}

struct SuggestionsView: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    @ScaledMetric(relativeTo: .body) private var cardWidth: CGFloat = 160
    @State private var offered = OfferedMixes()

    var body: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 16) {
                Text("suggestions.lead").foregroundStyle(.secondary)
                ForEach(offered.shelves, id: \.name) { shelf in
                    Text(shelf.name).font(.title3.weight(.semibold)).accessibilityAddTraits(.isHeader)
                    LazyVGrid(columns: [GridItem(.adaptive(minimum: cardWidth), spacing: 16, alignment: .top)], spacing: 20) {
                        ForEach(shelf.mixes) { mix in
                            SuggestedMixCard(
                                mix: mix,
                                covers: (offered.covers[mix.id] ?? []).compactMap { library.artworkTrack($0) },
                                open: { navigation.open(.mix(mix.id)) }
                            )
                        }
                    }
                }
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 20)
        }
        .navigationTitle("tools.suggestions")
        .background {
            CatalogReading {
                guard let found = await OfferedMixes.read(in: library), found != offered else { return }
                offered = found
            }
        }
    }
}

struct MixView: View {
    private static let corner: CGFloat = 20

    @Environment(Library.self) private var library
    let id: String
    @State private var mix: SuggestedMix?
    @State private var resolved = false
    @State private var tracks: [Track] = []
    @State private var covers: [Track] = []
    @State private var read = false
    @State private var failure: String?

    var body: some View {
        Group {
            if let mix {
                List {
                    CollectionHeader(title: mix.title, subtitle: mix.detail, tracks: tracks, outline: .rounded(Self.corner)) {
                        MosaicArtwork(covers: covers, mark: mix.mark, name: mix.title, corner: Self.corner)
                    }
                    if let failure {
                        ReadFailure(text: failure) { Task { await load(mix) } }
                    } else if read, tracks.isEmpty {
                        Text("mix.empty").foregroundStyle(.secondary)
                    }
                    ForEach(tracks, id: \.id) { track in
                        TrackRow(track: track) {
                            library.play(tracks, selected: track)
                        }
                        .listRowBackground(Color.clear)
                    }
                }
                .listStyle(.plain)
                .background(BackdropView(track: tracks.first, moving: false))
                .scrollContentBackground(.hidden)
                .navigationTitle(mix.title)
                .navigationBarTitleDisplayMode(.inline)
                .toolbar { CollectionMenu(tracks: tracks, keeping: mix.kept, keepingAs: mix.title) }
            } else if resolved {
                ContentUnavailableView("mix.gone", systemImage: "square.stack")
            } else {
                ProgressView().accessibilityLabel("mix.reading")
            }
        }
        .background { CatalogReading(subject: id) { await resolveAndLoad() } }
    }

    private func resolveAndLoad() async {
        var found = SuggestedMix.yours.first { $0.id == id }
        if found == nil, let offered = await OfferedMixes.read(in: library) {
            found = offered.named(id)
            resolved = true
            mix = found
        } else if found != nil {
            resolved = true
            mix = found
        }
        if let found { await load(found) }
    }

    private func load(_ mix: SuggestedMix) async {
        guard let native = try? await library.player() else { return }
        do {
            let (rows, pictured) = try await Work.onScreen {
                (try mix.tracks(in: native), try mix.covers(in: native))
            }
            failure = nil
            if tracks != rows { tracks = rows }
            let pictures = pictured.compactMap { library.artworkTrack($0) }
            if covers != pictures { covers = pictures }
            if !read { read = true }
        } catch {
            failure = explained(error)
        }
    }
}

struct SuggestedMixCard: View {
    private static let corner: CGFloat = 10

    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    let mix: SuggestedMix
    let covers: [Track]
    var shelved = false
    let open: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Button(action: open) {
                MosaicArtwork(covers: covers, mark: mix.mark, name: mix.title, corner: Self.corner)
            }
            .buttonStyle(.plain)
            .accessibilityHidden(true)
            HStack(alignment: .top, spacing: 8) {
                Button(action: open) {
                    VStack(alignment: .leading, spacing: 2) {
                        Text(mix.title).font(.subheadline.weight(.semibold)).titleLines()
                        detail
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .contentShape(.rect)
                }
                .buttonStyle(.plain)
                .accessibilityHint("mix.open.hint")
                Button(said("mix.play.named", mix.title), systemImage: "play.fill") { play(shuffled: false) }
                    .labelStyle(.iconOnly)
                    .buttonStyle(.glass)
                    .buttonBorderShape(.circle)
            }
        }
        .contextMenu {
            Button("common.play", systemImage: "play.fill") { play(shuffled: false) }
            Button("common.shuffle", systemImage: "shuffle") { play(shuffled: true) }
            Button("mix.show-songs", systemImage: "music.note.list", action: open)
        }
    }

    @ViewBuilder
    private var detail: some View {
        let words = Text(mix.detail).font(.caption).foregroundStyle(.secondary)
        if shelved {
            words.reservedLines(2)
        } else {
            words.titleLines(2)
        }
    }

    private func play(shuffled: Bool) {
        let mix = mix
        Task {
            do {
                let native = try await library.player()
                let tracks = try await Work.onScreen { try mix.tracks(in: native) }
                guard !tracks.isEmpty else {
                    library.notice(said("mix.empty.named", mix.title))
                    return
                }
                navigation.feelQueued()
                if shuffled { library.shuffle(tracks) } else { library.play(tracks, at: 0) }
            } catch {
                library.report(error)
            }
        }
    }
}

struct FilterSheet: View {
    @Environment(Library.self) private var library
    @Environment(\.dismiss) private var dismiss
    @State private var filter = SongBrowsing.defaultFilter
    @State private var genres: [String] = []
    @State private var artists: [String] = []
    @State private var albums: [String] = []
    @State private var discarding = false
    @State private var seeded = false

    var body: some View {
        NavigationStack {
            Form {
                CataloguePicker(title: said("filter.genre"), choice: $filter.genre, names: genres)
                CataloguePicker(title: said("filter.artist"), choice: $filter.artist, names: artists)
                CataloguePicker(title: said("filter.album"), choice: $filter.album, names: albums)
                Toggle("filter.favourites-only", isOn: $filter.favourites)
                Toggle("filter.unplayed-only", isOn: $filter.unplayed)
                Toggle("filter.high-resolution-only", isOn: Binding(
                    get: { filter.minimumRate >= 88200 },
                    set: { filter.minimumRate = $0 ? 88200 : 0 }
                ))
                Stepper(filter.minimumBits == 0 ? said("filter.bits.any") : said("filter.bits.at-least", Int(filter.minimumBits)), value: $filter.minimumBits, in: 0...32, step: 8)
                Picker("filter.sort-by", selection: $filter.order) {
                    ForEach(FilterChip.orderNames, id: \.1) { Text($0.1).tag($0.0) }
                }
                Toggle("filter.descending", isOn: $filter.descending)
                Section {
                    Button("filter.reset", role: .destructive) { filter = SongBrowsing.defaultFilter }
                        .disabled(filter == SongBrowsing.defaultFilter)
                }
            }
            .navigationTitle("filter.title")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button(role: .close) {
                        if filter == library.filter { dismiss() } else { discarding = true }
                    }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("filter.apply") {
                        library.setFilter(filter)
                        dismiss()
                    }
                }
            }
            .onAppear {
                guard !seeded else { return }
                seeded = true
                filter = library.filter
            }
            .task { await readNames() }
        }
        .keepingEditsUnlessDiscarded(filter != library.filter, asking: $discarding) { dismiss() }
    }

    private func readNames() async {
        let tracks = library.allTracks
        let titles = library.albums.map(\.title)
        let (genres, artists, albums) = await Task.detached(priority: .userInitiated) {
            (FilterSheet.distinct(tracks.map(\.genre)), FilterSheet.distinct(tracks.map(\.artist)), FilterSheet.distinct(titles))
        }.value
        self.genres = genres
        self.artists = artists
        self.albums = albums
    }

    nonisolated private static func distinct(_ names: [String]) -> [String] {
        var seen = Set<String>()
        var folded = Set<String>()
        return names
            .filter { !$0.isEmpty && seen.insert($0).inserted && folded.insert($0.searchFolded).inserted }
            .sorted { $0.localizedStandardCompare($1) == .orderedAscending }
    }
}

struct CataloguePicker: View {
    let title: String
    @Binding var choice: String
    let names: [String]

    var body: some View {
        NavigationLink {
            CatalogueChoices(title: title, choice: $choice, names: names)
        } label: {
            LabeledContent(title, value: choice.isEmpty ? said("filter.any") : choice)
        }
    }
}

private struct CatalogueChoices: View {
    @Environment(\.dismiss) private var dismiss
    let title: String
    @Binding var choice: String
    let names: [String]

    var body: some View {
        List {
            row(said("filter.any"), choosing: "")
            if !choice.isEmpty, !names.contains(choice) {
                row(choice, choosing: choice)
            }
            ForEach(names, id: \.self) { row($0, choosing: $0) }
        }
        .navigationTitle(title)
        .navigationBarTitleDisplayMode(.inline)
    }

    private func row(_ label: String, choosing name: String) -> some View {
        Button {
            choice = name
            dismiss()
        } label: {
            HStack {
                Text(label).foregroundStyle(.primary)
                Spacer(minLength: 0)
                if choice == name {
                    Image(systemName: "checkmark")
                        .fontWeight(.semibold)
                        .foregroundStyle(.tint)
                        .accessibilityHidden(true)
                }
            }
            .contentShape(.rect)
        }
        .accessibilityAddTraits(choice == name ? .isSelected : [])
    }
}
