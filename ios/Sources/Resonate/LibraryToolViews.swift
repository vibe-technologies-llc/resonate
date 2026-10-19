import ResonateNative
import SwiftUI

struct ToolRow: View {
    let title: String
    let detail: String
    let symbol: String

    var body: some View {
        Label {
            VStack(alignment: .leading, spacing: 2) {
                Text(title)
                Text(detail).font(.subheadline).foregroundStyle(.secondary)
            }
        } icon: {
            Image(systemName: symbol).foregroundStyle(.tint)
        }
    }
}

struct LibraryToolsView: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation

    var body: some View {
        let current = library.playingTrack
        ThemedList {
            Section("Your library") {
                NavigationLink(value: Destination.playlists) {
                    ToolRow(title: "Playlists", detail: "Listen to your collections", symbol: "music.note.list")
                }
                NavigationLink(value: Destination.suggestions) {
                    ToolRow(title: "Suggestions", detail: "Rediscover music from your library", symbol: "sparkles")
                }
                NavigationLink(value: Destination.statistics) {
                    ToolRow(title: "Statistics", detail: "Your listening history and most played music", symbol: "chart.bar")
                }
                NavigationLink(value: Destination.identicalSongs) {
                    ToolRow(title: "Identical songs", detail: "Find files held twice and keep one of each", symbol: "square.on.square")
                }
                Button { navigation.filters = true } label: {
                    ToolRow(title: "Filter and sort", detail: "Genre, artist, audio quality and more", symbol: "line.3.horizontal.decrease.circle")
                }
            }
            Section("Discover") {
                Button { navigation.searchOnline() } label: {
                    ToolRow(title: "Online search", detail: "Find songs, artists, albums and shared music", symbol: "globe")
                }
                Button { navigation.listening = true } label: {
                    ToolRow(title: "Listen", detail: "Name the song playing around you", symbol: "ear")
                }
                NavigationLink(value: Destination.wanted) {
                    ToolRow(title: "Saved for later", detail: "Recordings you want to add to your library", symbol: "bookmark")
                }
                RefreshArtworkRow()
                if library.features.organiseUndo != nil {
                    Button { library.features.putFilesBack() } label: {
                        ToolRow(title: "Put files back", detail: "Return the songs you last organised to where they were", symbol: "arrow.uturn.backward.circle")
                    }
                    .disabled(library.features.organising)
                }
                if library.features.tagsUndoable {
                    Button { library.features.undoTags() } label: {
                        ToolRow(title: "Undo tag edit", detail: "Put back the tags you last saved", symbol: "arrow.uturn.backward")
                    }
                }
            }
            Section {
                NavigationLink(value: current.map { _ in Destination.lyrics(nil) }) {
                    ToolRow(title: "Lyrics", detail: "Follow the words as they play", symbol: "quote.bubble")
                }
                NavigationLink(value: current.map { _ in Destination.visualizer }) {
                    ToolRow(title: "Visualizer", detail: "Watch the live waveform", symbol: "waveform")
                }
                NavigationLink(value: current.map { _ in Destination.analysis(nil) }) {
                    ToolRow(title: "Analysis", detail: "Loudness, spectrum and audio quality", symbol: "waveform.path.ecg")
                }
                NavigationLink(value: current.map(Destination.inspector)) {
                    ToolRow(title: "Inspector", detail: "Artwork, tags and source details", symbol: "info.circle")
                }
            } header: {
                Text("Current song")
            } footer: {
                if current == nil { Text("Play a song to use these tools.") }
            }
            .disabled(current == nil)
            Section("Preferences") {
                Button { navigation.tab = .settings } label: {
                    ToolRow(title: "Settings", detail: "Sound, music folders and connected services", symbol: "gearshape")
                }
            }
        }
        .buttonStyle(.plain)
        .navigationTitle("Library tools")
    }
}

struct FoldersView: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    @State private var removing: MusicFolder?
    @State private var organising: MusicFolder?

    var body: some View {
        ThemedList {
            Section {
                Text("Music is read through the folders you grant. Songs you put in Resonate's folder in the Files app are read from On this iPhone.")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            }
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
                            Button("Remove access", role: .destructive) { removing = folder }
                        }
                    }
                }
            }
            Section {
                Button("Add music folder", systemImage: "folder.badge.plus") { navigation.choosingFolder = true }
            }
        }
        .navigationTitle("Music folders")
        .sheet(item: $organising) { OrganiseSheet(folder: $0) }
        .task { library.folderList = await library.folders.refreshed() }
        .confirmationDialog(
            "Remove \(removing?.name ?? "this folder")?",
            isPresented: Binding(get: { removing != nil }, set: { if !$0 { removing = nil } }),
            titleVisibility: .visible,
            presenting: removing
        ) { folder in
            Button("Remove access", role: .destructive) { library.removeFolder(folder) }
        } message: { _ in
            Text("Your music files stay where they are. Its songs are hidden until you add the folder again.")
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
                VStack(alignment: .leading, spacing: 2) {
                    Text(folder.name)
                    Text(standing)
                        .font(.caption)
                        .foregroundStyle(folder.readable ? Color.secondary : Color.red)
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
        Button("Rescan", systemImage: "arrow.clockwise", action: rescan)
            .disabled(!folder.readable || scanning)
        Button("Organise", systemImage: "folder.badge.gearshape", action: organise)
            .disabled(!folder.writable || scanning)
        if folder.id != Folders.local {
            Button("Remove access", systemImage: "xmark.circle", role: .destructive, action: remove)
        }
    }

    private var standing: String {
        if !folder.readable { return "Unavailable · choose this folder again to restore access" }
        return folder.writable ? "Access available" : "Read-only access · songs from your devices cannot be kept here"
    }
}

struct SuggestedMix: Identifiable {
    let title: String
    let detail: String
    let filter: TrackFilter
    let accents: MixAccents
    let mark: String

    var id: String { title }

    static func offered(topArtist: String?) -> [SuggestedMix] {
        let base = defaultTrackFilter()
        var unplayed = base
        unplayed.unplayed = true
        var favourites = base
        favourites.favourites = true
        var played = base
        played.order = .plays
        played.descending = true
        played.played = true
        var recent = base
        recent.order = .played
        recent.descending = true
        recent.played = true
        var added = base
        added.order = .added
        added.descending = true
        var high = base
        high.minimumRate = 88200
        var mixes = [
            SuggestedMix(title: "Never heard", detail: "Songs you have not played yet", filter: unplayed, accents: MixAccents(from: .teal, to: .blue), mark: "sparkles"),
            SuggestedMix(title: "Your favourites", detail: "Every song you hearted", filter: favourites, accents: MixAccents(from: .mauve, to: .red), mark: "heart.fill"),
            SuggestedMix(title: "Most played", detail: "What you come back to", filter: played, accents: MixAccents(from: .red, to: .peach), mark: "chart.bar.fill"),
            SuggestedMix(title: "Recently played", detail: "What you listened to last", filter: recent, accents: MixAccents(from: .amber, to: .peach), mark: "clock.arrow.circlepath"),
            SuggestedMix(title: "Recently added", detail: "The newest in your library", filter: added, accents: MixAccents(from: .green, to: .teal), mark: "clock.fill"),
            SuggestedMix(title: "High-resolution music", detail: "88.2 kHz and above", filter: high, accents: MixAccents(from: .green, to: .blue), mark: "waveform"),
        ]
        if let topArtist, !topArtist.isEmpty {
            var artist = base
            artist.artist = topArtist
            mixes.append(SuggestedMix(title: "More from \(topArtist)", detail: "From your most played artist", filter: artist, accents: namedAccents(name: topArtist), mark: "music.mic"))
        }
        return mixes
    }
}

struct SuggestionsView: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    @ScaledMetric(relativeTo: .body) private var cardWidth: CGFloat = 160
    @State private var covers: [String: [UInt64]] = [:]

    var body: some View {
        let mixes = SuggestedMix.offered(topArtist: library.features.listeningDetail?.artists.first?.name)
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                Text("Mixes from your library and listening history").foregroundStyle(.secondary)
                LazyVGrid(columns: [GridItem(.adaptive(minimum: cardWidth), spacing: 16, alignment: .top)], spacing: 20) {
                    ForEach(mixes) { mix in
                        SuggestedMixCard(
                            mix: mix,
                            covers: (covers[mix.id] ?? []).compactMap { library.artworkTrack($0) },
                            open: { open(mix) }
                        )
                    }
                }
            }
            .padding(20)
        }
        .navigationTitle("Suggestions")
        .onAppear { library.features.loadStatistics() }
        .task(id: "\(library.collectionGeneration):\(mixes.map(\.id).joined())") { await loadCovers(mixes) }
    }

    private func open(_ mix: SuggestedMix) {
        library.setFilter(mix.filter)
        navigation.songsGrouping = .songs
        navigation.tab = .songs
        navigation.paths[.songs] = []
    }

    private func loadCovers(_ mixes: [SuggestedMix]) async {
        guard let native = try? await library.player() else { return }
        let asked = mixes.map { ($0.id, $0.filter) }
        guard let found = try? await Work.aside({
            Dictionary(uniqueKeysWithValues: try asked.map { ($0.0, try native.picturedBy(filter: $0.1)) })
        }) else { return }
        covers = found
    }
}

struct SuggestedMixCard: View {
    @Environment(Library.self) private var library
    let mix: SuggestedMix
    let covers: [Track]
    let open: () -> Void
    @State private var played = 0

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Button(action: open) {
                MosaicArtwork(covers: covers, accents: mix.accents, mark: mix.mark, name: mix.title, corner: 16)
                    .background { HeaderShadow(outline: .rounded(16)) }
            }
            .buttonStyle(.plain)
            .accessibilityLabel(mix.title)
            .accessibilityHint("Shows the songs in this mix")
            HStack(alignment: .top, spacing: 8) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(mix.title).font(.subheadline.weight(.semibold)).lineLimit(1)
                    Text(mix.detail).font(.caption).foregroundStyle(.secondary).lineLimit(2)
                }
                Spacer(minLength: 0)
                Button("Play \(mix.title)", systemImage: "play.fill") { play(shuffled: false) }
                    .labelStyle(.iconOnly)
                    .buttonStyle(.glass)
                    .buttonBorderShape(.circle)
            }
        }
        .contextMenu {
            Button("Play", systemImage: "play.fill") { play(shuffled: false) }
            Button("Shuffle", systemImage: "shuffle") { play(shuffled: true) }
            Button("Show songs", systemImage: "music.note.list", action: open)
        }
        .sensoryFeedback(.success, trigger: played)
    }

    private func play(shuffled: Bool) {
        let filter = mix.filter
        Task {
            do {
                let native = try await library.player()
                let tracks = try await Work.aside { try Library.everyPage { try native.filteredTracks(filter: filter, offset: $0, limit: Library.page) } }
                guard !tracks.isEmpty else {
                    library.notice("Nothing in \(mix.title) yet")
                    return
                }
                played += 1
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
    @State private var filter = defaultTrackFilter()
    @State private var genres: [String] = []
    @State private var artists: [String] = []
    @State private var albums: [String] = []

    var body: some View {
        NavigationStack {
            Form {
                CataloguePicker(title: "Genre", choice: $filter.genre, names: genres)
                CataloguePicker(title: "Artist", choice: $filter.artist, names: artists)
                CataloguePicker(title: "Album", choice: $filter.album, names: albums)
                Toggle("Favourites only", isOn: $filter.favourites)
                Toggle("Unplayed only", isOn: $filter.unplayed)
                Toggle("High-resolution only", isOn: Binding(
                    get: { filter.minimumRate >= 88200 },
                    set: { filter.minimumRate = $0 ? 88200 : 0 }
                ))
                Stepper(filter.minimumBits == 0 ? "Any bit depth" : "At least \(filter.minimumBits)-bit", value: $filter.minimumBits, in: 0...32, step: 8)
                Picker("Sort by", selection: $filter.order) {
                    ForEach(FilterChip.orderNames, id: \.1) { Text($0.1).tag($0.0) }
                }
                Toggle("Descending", isOn: $filter.descending)
                Section {
                    Button("Reset", role: .destructive) { filter = defaultTrackFilter() }
                        .disabled(filter == defaultTrackFilter())
                }
            }
            .navigationTitle("Filter and sort")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button(role: .close) { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Apply") {
                        library.setFilter(filter)
                        dismiss()
                    }
                }
            }
            .onAppear { filter = library.filter }
            .task { await readNames() }
        }
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
        Picker(title, selection: $choice) {
            Text("Any").tag("")
            if !choice.isEmpty, !names.contains(choice) {
                Text(choice).tag(choice)
            }
            ForEach(names, id: \.self) { Text($0).tag($0) }
        }
        .pickerStyle(.navigationLink)
    }
}
