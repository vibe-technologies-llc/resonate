import ResonateNative
import SwiftUI

enum OnlineMode: String, CaseIterable, Identifiable {
    case songs = "Songs"
    case albums = "Albums"
    case artists = "Artists"

    var id: String { rawValue }
}

struct OnlineAsked: Hashable {
    static let settle: Duration = .milliseconds(450)
    static let shortest = 3
    private static let linkPrefixes = ["http://", "https://", "spotify:"]

    let text: String
    let mode: OnlineMode?

    init(_ typed: String, in mode: OnlineMode) {
        text = typed.trimmingCharacters(in: .whitespacesAndNewlines)
        self.mode = Self.isLink(text) ? nil : mode
    }

    init(text: String, mode: OnlineMode?) {
        self.text = text
        self.mode = mode
    }

    var isLink: Bool { mode == nil }

    static func isLink(_ text: String) -> Bool {
        let lowered = text.lowercased()
        return linkPrefixes.contains { lowered.hasPrefix($0) }
    }
}

enum OnlineAnswer {
    case songs([SearchSong])
    case albums([SearchAlbum])
    case artists([SearchArtist])
    case link(OnlineCollection)

    static func asked(_ asked: OnlineAsked, of native: MobilePlayer) throws -> OnlineAnswer {
        switch asked.mode {
        case nil: .link(try native.openOnlineLink(text: asked.text))
        case .songs: .songs(try native.searchOnline(text: asked.text))
        case .albums: .albums(try native.searchAlbumsOnline(text: asked.text))
        case .artists: .artists(try native.searchArtistsOnline(text: asked.text))
        }
    }

    var isEmpty: Bool {
        switch self {
        case .songs(let songs): songs.isEmpty
        case .albums(let albums): albums.isEmpty
        case .artists(let artists): artists.isEmpty
        case .link(let collection): collection.songs.isEmpty && collection.releases.isEmpty && collection.artists.isEmpty
        }
    }
}

struct OnlineResults {
    let asked: OnlineAsked
    let answer: OnlineAnswer
}

struct OnlineAnswers {
    private static let keptPerMode = 64

    private var kept: [OnlineAsked: OnlineAnswer] = [:]
    private var spellings: [OnlineMode?: [String]] = [:]

    func answer(for asked: OnlineAsked) -> OnlineAnswer? {
        kept[asked]
    }

    mutating func keep(_ answer: OnlineAnswer, for asked: OnlineAsked) {
        var recent = spellings[asked.mode, default: []].filter { $0 != asked.text }
        recent.append(asked.text)
        while recent.count > Self.keptPerMode {
            kept[OnlineAsked(text: recent.removeFirst(), mode: asked.mode)] = nil
        }

        spellings[asked.mode] = recent
        kept[asked] = answer
    }
}

struct OnlineView: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    @State private var presented = false
    @State private var actions = OnlineSongActions()

    var body: some View {
        @Bindable var features = library.features
        List {
            if !features.online {
                Section {
                    Text("Online features are disabled. Turn them on in Settings to search and download music.")
                    Button("Open Settings") { navigation.tab = .settings }
                }
            } else {
                DownloadProgress()
                OnlineFindings(actions: $actions)
            }
        }
        .navigationTitle("Online music search")
        .searchable(
            text: $features.onlineQuery,
            isPresented: $presented,
            placement: .navigationBarDrawer(displayMode: .always),
            prompt: "Song, album, artist or shared link"
        )
        .searchScopes($features.onlineMode, activation: .onSearchPresentation) {
            ForEach(OnlineMode.allCases) { Text($0.rawValue).tag($0) }
        }
        .textInputAutocapitalization(.never)
        .autocorrectionDisabled()
        .toolbar {
            if features.searching {
                ToolbarItem(placement: .topBarTrailing) { ProgressView() }
            }
        }
        .onlineSongActions($actions)
        .onAppear { if features.online, features.onlineQuery.isEmpty { presented = true } }
    }
}

struct OnlineFindings: View {
    @Environment(Library.self) private var library
    @Binding var actions: OnlineSongActions

    var body: some View {
        let features = library.features
        let asked = OnlineAsked(features.onlineQuery, in: features.onlineMode)
        if asked.text.isEmpty {
            ContentUnavailableView(
                "Search online",
                systemImage: "globe",
                description: Text("Find songs, artists, albums and shared music. Paste a shared link to open it.")
            )
            .listRowSeparator(.hidden)
        } else if !asked.isLink, asked.text.count < OnlineAsked.shortest {
            Text("Type at least 3 letters").foregroundStyle(.secondary)
        } else if let results = features.onlineResults {
            if !results.answer.isEmpty {
                OnlineAnswerSections(answer: results.answer, actions: $actions)
            } else if results.asked == asked, !features.searching {
                ContentUnavailableView(
                    asked.isLink ? "No music could be read from this link" : "No results for “\(asked.text)”",
                    systemImage: asked.isLink ? "link" : "magnifyingglass",
                    description: Text(asked.isLink ? "Check the link, or search by name instead." : "Check the spelling, or try fewer words.")
                )
                .listRowSeparator(.hidden)
            }
        }
    }
}

struct OnlineAnswerSections: View {
    let answer: OnlineAnswer
    @Binding var actions: OnlineSongActions

    var body: some View {
        switch answer {
        case .songs(let songs):
            OnlineSongsSection(songs: songs, actions: $actions)
        case .albums(let albums):
            Section {
                ForEach(albums, id: \.id) { album in
                    NavigationLink { OnlineAlbumPage(album: album) } label: { OnlineAlbumRow(album: album) }
                }
            }
        case .artists(let artists):
            Section {
                ForEach(artists, id: \.id) { artist in
                    NavigationLink { OnlineArtistPage(artist: artist) } label: { OnlineArtistRow(artist: artist) }
                }
            }
        case .link(let collection):
            let heading = collection.name.isEmpty ? "Shared music" : collection.name
            if !collection.artists.isEmpty {
                Section(heading) {
                    ForEach(collection.artists, id: \.id) { artist in
                        NavigationLink { OnlineArtistPage(artist: artist) } label: { OnlineArtistRow(artist: artist) }
                    }
                }
            }
            if !collection.releases.isEmpty {
                Section(heading) {
                    ForEach(collection.releases, id: \.id) { release in
                        NavigationLink { OnlineReleasePage(release: release) } label: { OnlineReleaseRow(release: release) }
                    }
                }
            }
            if !collection.songs.isEmpty {
                OnlineSongsSection(songs: collection.songs, heading: heading, collected: true, actions: $actions)
            }
        }
    }
}

struct OnlineSongActions {
    var importing = false
    var importName = ""
    var importedSongs: [SearchSong] = []
}

extension View {
    func onlineSongActions(_ actions: Binding<OnlineSongActions>) -> some View {
        modifier(OnlineSongPresentations(actions: actions))
    }
}

struct OnlineSongPresentations: ViewModifier {
    @Environment(Library.self) private var library
    @Binding var actions: OnlineSongActions

    func body(content: Content) -> some View {
        content
            .downloadDestinationPrompt()
            .alert("Import matching local songs", isPresented: $actions.importing) {
                TextField("Playlist name", text: $actions.importName)
                Button("Cancel", role: .cancel) {}
                Button("Import") {
                    let name = actions.importName.trimmingCharacters(in: .whitespacesAndNewlines)
                    library.features.importPlaylist(name.isEmpty ? "Shared music" : name, actions.importedSongs)
                }
            } message: {
                Text("Songs already in your library are added in the shared order. Save the remaining recordings for later to download them.")
            }
    }
}

struct OnlineSongsSection: View {
    @Environment(Library.self) private var library
    let songs: [SearchSong]
    var heading = ""
    var collected = false
    @Binding var actions: OnlineSongActions

    var body: some View {
        let features = library.features
        Section {
            if collected, !songs.isEmpty {
                Button("Save all for later", systemImage: "bookmark") { features.saveWanted(songs) }
                Button("Import local matches as playlist", systemImage: "text.badge.plus") {
                    actions.importName = heading
                    actions.importedSongs = songs
                    actions.importing = true
                }
            }
            ForEach(songs.indices, id: \.self) { index in
                OnlineSongRow(song: songs[index]) { features.downloads.enqueue([songs[index]]) }
            }
        } header: {
            if !heading.isEmpty { Text(heading) }
        }
    }
}

struct OnlinePlaceholder: View {
    let symbol: String
    var circle = false

    var body: some View {
        ZStack {
            Rectangle().fill(.quaternary)
            Image(systemName: symbol).foregroundStyle(.secondary)
        }
        .frame(width: 44, height: 44)
        .clipShape(circle ? AnyShape(Circle()) : AnyShape(RoundedRectangle(cornerRadius: 8, style: .continuous)))
        .accessibilityHidden(true)
    }
}

struct OnlineSongRow: View {
    @Environment(Library.self) private var library
    let song: SearchSong
    let download: () -> Void

    var body: some View {
        let features = library.features
        HStack(spacing: 12) {
            OnlinePlaceholder(symbol: "music.note")
            VStack(alignment: .leading, spacing: 2) {
                Text(song.title).font(.body.weight(.medium)).lineLimit(1)
                Text([song.artist, song.album].filter { !$0.isEmpty }.joined(separator: " · "))
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                HStack(spacing: 6) {
                    if let duration = song.durationMs {
                        Text(clock(duration)).font(.caption.monospacedDigit()).foregroundStyle(.secondary)
                    }
                    if song.held {
                        Label("In your library", systemImage: "checkmark.circle.fill")
                            .font(.caption)
                            .foregroundStyle(.tint)
                    }
                }
            }
            Spacer(minLength: 0)
            Menu {
                Button("Download", systemImage: "arrow.down.circle", action: download)
                    .disabled(features.downloads.holds(song))
                Button("Save for later", systemImage: "bookmark") { features.saveWanted([song]) }
            } label: {
                Image(systemName: "ellipsis.circle").font(.title3)
            }
            .accessibilityLabel("Song actions")
        }
    }
}

struct OnlineAlbumRow: View {
    let album: SearchAlbum

    var body: some View {
        HStack(spacing: 12) {
            OnlinePlaceholder(symbol: "square.stack")
            VStack(alignment: .leading, spacing: 2) {
                Text(album.title).font(.body.weight(.medium)).lineLimit(1)
                Text([album.artist, album.date].filter { !$0.isEmpty }.joined(separator: " · "))
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
        }
    }
}

struct OnlineArtistRow: View {
    let artist: SearchArtist

    var body: some View {
        HStack(spacing: 12) {
            OnlinePlaceholder(symbol: "music.mic", circle: true)
            VStack(alignment: .leading, spacing: 2) {
                Text(artist.name).font(.body.weight(.medium)).lineLimit(1)
                if !artist.kind.isEmpty {
                    Text(artist.kind).font(.subheadline).foregroundStyle(.secondary)
                }
            }
        }
    }
}

struct OnlineReleaseRow: View {
    let release: SearchRelease

    var body: some View {
        HStack(spacing: 12) {
            OnlinePlaceholder(symbol: "opticaldisc")
            VStack(alignment: .leading, spacing: 2) {
                Text(release.title).font(.body.weight(.medium)).lineLimit(1)
                Text([release.date, release.country, release.songs > 0 ? counted(Int(release.songs), "song") : "View track list"]
                    .filter { !$0.isEmpty }
                    .joined(separator: " · "))
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
        }
    }
}

struct OnlineArtistPage: View {
    @Environment(Library.self) private var library
    let artist: SearchArtist
    @State private var details: ArtistInformation?
    @State private var albums: [SearchAlbum] = []
    @State private var next: UInt32?
    @State private var read = false
    @State private var readingMore = false

    var body: some View {
        List {
            if let details { ArtistInformationSection(artist: details) }
            Section("Albums") {
                ForEach(albums, id: \.id) { album in
                    NavigationLink { OnlineAlbumPage(album: album) } label: { OnlineAlbumRow(album: album) }
                }
                if let next {
                    Button(readingMore ? "Reading more releases…" : "More releases") {
                        Task { await readMore(from: next) }
                    }
                    .disabled(readingMore)
                }
                if read, albums.isEmpty {
                    Text("No albums by this artist were found.").foregroundStyle(.secondary)
                }
            }
        }
        .overlay { if !read { ProgressView() } }
        .navigationTitle(artist.name)
        .navigationBarTitleDisplayMode(.inline)
        .task { await readArtist() }
    }

    private func readArtist() async {
        guard !read else { return }
        let id = artist.id
        let features = library.features

        async let discography = features.onlineAnswer { try $0.artistAlbums(id: id, from: 0) }
        async let information = features.onlineAnswer { try $0.onlineArtist(id: id) }
        let page = await discography
        albums = page?.albums ?? []
        next = page?.next
        read = true
        details = await information ?? nil
    }

    private func readMore(from offset: UInt32) async {
        let id = artist.id
        readingMore = true
        defer { readingMore = false }

        guard let page = await library.features.onlineAnswer({ try $0.artistAlbums(id: id, from: offset) }) else { return }
        var seen = Set(albums.map(\.id))
        albums += page.albums.filter { seen.insert($0.id).inserted }
        next = page.next
    }
}

struct OnlineAlbumPage: View {
    @Environment(Library.self) private var library
    let album: SearchAlbum
    @State private var releases: [SearchRelease] = []
    @State private var read = false

    var body: some View {
        List {
            Section {
                VStack(alignment: .leading, spacing: 4) {
                    Text(album.title).font(.title2.weight(.bold))
                    Text([album.artist, album.date].filter { !$0.isEmpty }.joined(separator: " · "))
                        .foregroundStyle(.secondary)
                }
            }
            Section("Releases") {
                ForEach(releases, id: \.id) { release in
                    NavigationLink { OnlineReleasePage(release: release) } label: { OnlineReleaseRow(release: release) }
                }
                if read, releases.isEmpty {
                    Text("No releases of this album were found.").foregroundStyle(.secondary)
                }
            }
        }
        .overlay { if !read { ProgressView() } }
        .navigationTitle("Album")
        .navigationBarTitleDisplayMode(.inline)
        .task { await readReleases() }
    }

    private func readReleases() async {
        guard !read else { return }
        let group = album.id
        releases = await library.features.onlineAnswer { try $0.onlineReleases(group: group) } ?? []
        read = true
    }
}

struct OnlineReleasePage: View {
    @Environment(Library.self) private var library
    let release: SearchRelease
    @State private var songs: [SearchSong] = []
    @State private var read = false
    @State private var actions = OnlineSongActions()

    var body: some View {
        List {
            DownloadProgress()
            OnlineSongsSection(songs: songs, heading: release.title, collected: true, actions: $actions)
            if read, songs.isEmpty {
                Text("No songs on this release were found.").foregroundStyle(.secondary)
            }
        }
        .overlay { if !read { ProgressView() } }
        .navigationTitle(release.title)
        .navigationBarTitleDisplayMode(.inline)
        .onlineSongActions($actions)
        .task { await readSongs() }
    }

    private func readSongs() async {
        guard !read else { return }
        let id = release.id
        songs = await library.features.onlineAnswer { try $0.releaseSongs(id: id) } ?? []
        read = true
    }
}

extension SearchSong: Identifiable {
    public var id: String { recording.isEmpty ? "\(title)\u{1F}\(artist)\u{1F}\(album)" : recording }
}
