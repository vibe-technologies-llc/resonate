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
        let features = library.features
        let current = library.snapshot?.current
        List {
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
                Button { navigation.filters = true } label: {
                    ToolRow(title: "Filter and sort", detail: "Genre, artist, audio quality and more", symbol: "line.3.horizontal.decrease.circle")
                }
            }
            Section("Discover") {
                NavigationLink(value: Destination.online) {
                    ToolRow(title: "Online search", detail: "Find songs, artists, albums and shared music", symbol: "globe")
                }
                NavigationLink(value: Destination.wanted) {
                    ToolRow(title: "Saved for later", detail: "Recordings you want to add to your library", symbol: "bookmark")
                }
                Button { features.enrich(library.tracks) } label: {
                    ToolRow(
                        title: "Enrich visible music",
                        detail: features.enriching
                            ? "Updating artwork and metadata…"
                            : !features.online ? "Enable online features in Settings" : "Look up covers, metadata and lyrics for \(counted(library.tracks.count, "song"))",
                        symbol: "wand.and.stars"
                    )
                }
                .disabled(features.enriching || !features.online)
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

    var body: some View {
        List {
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
        .task { library.folderList = await library.folders.refreshed() }
        .confirmationDialog(
            "Remove \(removing?.name ?? "this folder")?",
            isPresented: Binding(get: { removing != nil }, set: { if !$0 { removing = nil } }),
            titleVisibility: .visible,
            presenting: removing
        ) { folder in
            Button("Remove access", role: .destructive) { library.removeFolder(folder) }
        } message: { _ in
            Text("Your music files stay where they are. Cached library entries remain, but playback from this folder will need access again.")
        }
    }
}

struct FolderRow: View {
    let folder: MusicFolder
    let scanning: Bool
    let rescan: () -> Void
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
            HStack {
                Button("Rescan", systemImage: "arrow.clockwise", action: rescan)
                    .disabled(!folder.readable || scanning)
                if folder.id != Folders.local {
                    Button("Remove access", systemImage: "xmark.circle", role: .destructive, action: remove)
                }
            }
            .buttonStyle(.bordered)
            .controlSize(.small)
        }
    }

    private var standing: String {
        if !folder.readable { return "Unavailable · choose this folder again to restore access" }
        return folder.writable ? "Access available" : "Read-only access · songs from your devices cannot be kept here"
    }
}

struct StatisticsView: View {
    @Environment(Library.self) private var library
    @State private var days: UInt32 = 0
    private static let spans: [(UInt32, String)] = [(7, "Week"), (30, "Month"), (365, "Year"), (0, "All time")]

    var body: some View {
        let features = library.features
        let statistics = features.statistics
        List {
            Picker("Period", selection: $days) {
                ForEach(Self.spans, id: \.0) { Text($0.1).tag($0.0) }
            }
            .pickerStyle(.segmented)
            if let statistics {
                Section("Listening") {
                    LabeledContent("Plays", value: "\(statistics.plays)")
                    LabeledContent("Listened", value: Self.spoken(statistics.listenedMs))
                }
                Section("Your library") {
                    LabeledContent("Songs", value: "\(statistics.songs)")
                    LabeledContent("Albums", value: "\(statistics.albums)")
                    LabeledContent("Artists", value: "\(statistics.artists)")
                    LabeledContent("Library length", value: Self.spoken(statistics.durationMs))
                }
                Section("Most listened") {
                    if statistics.topTracks.isEmpty {
                        ContentUnavailableView(
                            "No plays yet",
                            systemImage: "chart.bar",
                            description: Text("Play a song for half its length, or four minutes, to count a play.")
                        )
                    }
                    ForEach(statistics.topTracks, id: \.id) { track in
                        TrackRow(track: track) { library.play(statistics.topTracks, selected: track) }
                    }
                }
            }
        }
        .overlay {
            if statistics == nil, features.statisticsLoading {
                ProgressView("Reading your listening history…")
            }
        }
        .toolbar {
            if statistics != nil, features.statisticsLoading {
                ProgressView()
            }
        }
        .navigationTitle("Statistics")
        .task(id: days) { features.loadStatistics(days: days) }
    }

    private static func spoken(_ milliseconds: UInt64) -> String {
        let minutes = Int(milliseconds / 60_000)
        let hours = minutes / 60
        let rest = minutes % 60
        if hours == 0 { return counted(rest, "minute") }
        if rest == 0 { return counted(hours, "hour") }
        return "\(counted(hours, "hour")) \(counted(rest, "minute"))"
    }
}

struct SuggestionsView: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation

    var body: some View {
        let mixes: [(String, TrackFilter)] = {
            let base = defaultTrackFilter()
            var unplayed = base
            unplayed.unplayed = true
            var favourites = base
            favourites.favourites = true
            var played = base
            played.order = .plays
            played.descending = true
            var added = base
            added.order = .added
            added.descending = true
            var high = base
            high.minimumRate = 88200
            return [
                ("Never heard", unplayed),
                ("Your favourites", favourites),
                ("Most played", played),
                ("Recently added", added),
                ("High-resolution music", high),
            ]
        }()
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                Text("Mixes from your library and listening history").foregroundStyle(.secondary)
                ForEach(mixes, id: \.0) { title, filter in
                    mix(title) { library.setFilter(filter) }
                }
                if let artist = library.features.statistics?.topTracks.first?.artist, !artist.isEmpty {
                    mix("More from \(artist)") {
                        var filter = defaultTrackFilter()
                        filter.artist = artist
                        library.setFilter(filter)
                    }
                }
            }
            .padding(20)
        }
        .navigationTitle("Suggestions")
        .onAppear { library.features.loadStatistics() }
    }

    private func mix(_ title: String, apply: @escaping () -> Void) -> some View {
        Button {
            apply()
            navigation.tab = .songs
            navigation.paths[.songs] = []
        } label: {
            VStack(alignment: .leading, spacing: 6) {
                Text(title).font(.title3.weight(.semibold))
                Text("Explore this mix").foregroundStyle(.tint)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(20)
            .glassEffect(.regular.interactive(), in: .rect(cornerRadius: 22))
        }
        .buttonStyle(.plain)
    }
}

struct FilterSheet: View {
    @Environment(Library.self) private var library
    @Environment(\.dismiss) private var dismiss
    @State private var filter = defaultTrackFilter()
    private static let orders: [(TrackOrder, String)] = [
        (.title, "Title"), (.album, "Album"), (.artist, "Artist"), (.duration, "Duration"), (.added, "Added"), (.plays, "Plays"),
    ]

    var body: some View {
        NavigationStack {
            Form {
                TextField("Genre", text: $filter.genre)
                TextField("Artist", text: $filter.artist)
                TextField("Album", text: $filter.album)
                Toggle("Favourites only", isOn: $filter.favourites)
                Toggle("Unplayed only", isOn: $filter.unplayed)
                Toggle("High-resolution only", isOn: Binding(
                    get: { filter.minimumRate >= 88200 },
                    set: { filter.minimumRate = $0 ? 88200 : 0 }
                ))
                Stepper(filter.minimumBits == 0 ? "Any bit depth" : "At least \(filter.minimumBits)-bit", value: $filter.minimumBits, in: 0...32, step: 8)
                Picker("Sort by", selection: $filter.order) {
                    ForEach(Self.orders, id: \.1) { Text($0.1).tag($0.0) }
                }
                Toggle("Descending", isOn: $filter.descending)
            }
            .navigationTitle("Filter and sort")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Reset") {
                        library.setFilter(defaultTrackFilter())
                        dismiss()
                    }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Apply") {
                        library.setFilter(filter)
                        dismiss()
                    }
                }
            }
            .onAppear { filter = library.filter }
        }
    }
}
