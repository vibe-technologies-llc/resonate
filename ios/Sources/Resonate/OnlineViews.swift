import ResonateNative
import SwiftUI

enum OnlineMode: String, CaseIterable {
    case songs
    case albums
    case artists
}

struct OnlineAsked: Hashable {
    static let settle: Duration = .milliseconds(450)
    static let shortest = 3
    static let prompt = "Song, album, artist or shared link"
    private static let linkPrefixes = ["http://", "https://", "spotify:"]

    let text: String
    let mode: OnlineMode?

    static func chain(_ text: String) -> [OnlineAsked] {
        if isLink(text) { return [OnlineAsked(text: text, mode: nil)] }
        guard text.count >= shortest else { return [] }
        return OnlineMode.allCases.map { OnlineAsked(text: text, mode: $0) }
    }

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
}

struct OnlineFound {
    private(set) var text = ""
    private(set) var songs: [SearchSong]?
    private(set) var albums: [SearchAlbum]?
    private(set) var artists: [SearchArtist]?
    private(set) var link: OnlineCollection?

    init() {}

    init(text: String, answers: [OnlineAnswer]) {
        self.text = text
        for answer in answers { take(answer) }
    }

    func landing(_ answer: OnlineAnswer, for text: String) -> OnlineFound {
        var found = self.text == text ? self : OnlineFound(text: text, answers: [])
        found.take(answer)
        return found
    }

    var isWhole: Bool { link != nil || (songs != nil && albums != nil && artists != nil) }

    var isEmpty: Bool {
        if let link { return link.songs.isEmpty && link.releases.isEmpty && link.artists.isEmpty }
        return (songs ?? []).isEmpty && (albums ?? []).isEmpty && (artists ?? []).isEmpty
    }

    func awaited(for asked: String) -> OnlineMode? {
        guard text == asked else { return .songs }
        if songs == nil { return .songs }
        if albums == nil { return .albums }
        return artists == nil ? .artists : nil
    }

    private mutating func take(_ answer: OnlineAnswer) {
        switch answer {
        case .songs(let found): songs = found
        case .albums(let found): albums = found
        case .artists(let found): artists = found
        case .link(let found): link = found
        }
    }
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
    @State private var actions = OnlineSongActions()

    var body: some View {
        @Bindable var features = library.features
        List {
            OnlineSearchContent(actions: $actions).themedRows()
        }
        .listStyle(.plain)
        .navigationTitle("Online music search")
        .searchable(text: $features.onlineQuery, placement: .navigationBarDrawer(displayMode: .always), prompt: OnlineAsked.prompt)
        .textInputAutocapitalization(.never)
        .autocorrectionDisabled()
        .onlineSongActions($actions)
    }
}

struct OnlineOff: View {
    @Environment(Navigation.self) private var navigation

    var body: some View {
        ContentUnavailableView {
            Label("Online features are off", systemImage: "globe")
        } description: {
            Text("Turn them on in Settings to search for music online and download it.")
        } actions: {
            Button("Open Settings") { navigation.tab = .settings }
                .buttonStyle(.glass)
        }
        .listRowSeparator(.hidden)
    }
}

struct OnlineSearchContent: View {
    @Environment(Library.self) private var library
    @Binding var actions: OnlineSongActions

    var body: some View {
        let features = library.features
        let text = features.onlineQuery.trimmingCharacters(in: .whitespacesAndNewlines)
        let found = features.onlineFound
        let current = found.text == text

        if !features.online {
            OnlineOff()
        } else if text.isEmpty {
            ContentUnavailableView(
                "Search online",
                systemImage: "globe",
                description: Text("Find songs, artists, albums and shared music. Paste a shared link to open it.")
            )
            .listRowSeparator(.hidden)
        } else if OnlineAsked.chain(text).isEmpty {
            Text("Type at least 3 letters")
                .foregroundStyle(.secondary)
                .listRowSeparator(.hidden)
        } else if current, found.isWhole, found.isEmpty, !features.searching {
            OnlineNothingFound(text: text)
        } else {
            DownloadProgress()
            if features.searching, !current {
                OnlinePending(text: text, awaited: nil)
            }
            OnlineTopSections(found: found, actions: $actions)
            if features.searching, current {
                OnlinePending(text: text, awaited: found.awaited(for: text))
            }
        }
    }
}

struct OnlineNothingFound: View {
    let text: String

    var body: some View {
        let link = OnlineAsked.isLink(text)
        ContentUnavailableView(
            link ? "No music could be read from this link" : "No results for “\(text)”",
            systemImage: link ? "link" : "magnifyingglass",
            description: Text(link ? "Check the link, or search by name instead." : "Check the spelling, or try fewer words.")
        )
        .listRowSeparator(.hidden)
    }
}

struct OnlinePending: View {
    let text: String
    let awaited: OnlineMode?

    var body: some View {
        HStack(spacing: 10) {
            ProgressView()
            Text(label).foregroundStyle(.secondary)
        }
        .font(.subheadline)
        .listRowSeparator(.hidden)
        .accessibilityElement(children: .combine)
    }

    private var label: String {
        if OnlineAsked.isLink(text) { return "Opening the link…" }
        guard let awaited else { return "Searching…" }
        return "Searching \(awaited.rawValue)…"
    }
}

struct OnlineTopSections: View {
    private static let topSongs = 5
    private static let topCollections = 6

    @Environment(Library.self) private var library
    let found: OnlineFound
    @Binding var actions: OnlineSongActions

    var body: some View {
        if let link = found.link {
            OnlineLinkSections(collection: link, actions: $actions)
        }
        if let songs = found.songs, !songs.isEmpty {
            let shown = Array(songs.prefix(Self.topSongs))
            Section {
                ForEach(shown.indices, id: \.self) { index in
                    OnlineSongRow(song: shown[index]) { library.features.downloads.enqueue([shown[index]]) }
                }
            } header: {
                SeeAllLink(title: "Songs", more: songs.count > Self.topSongs) { OnlineSongsPage(songs: songs) }
            }
        }
        if let albums = found.albums, !albums.isEmpty {
            Section {
                ForEach(albums.prefix(Self.topCollections), id: \.id) { album in
                    NavigationLink { OnlineAlbumPage(album: album) } label: { OnlineAlbumRow(album: album) }
                    .themedRows()
                }
            } header: {
                SeeAllLink(title: "Albums", more: albums.count > Self.topCollections) { OnlineAlbumsPage(albums: albums) }
            }
        }
        if let artists = found.artists, !artists.isEmpty {
            Section {
                ForEach(artists.prefix(Self.topCollections), id: \.id) { artist in
                    NavigationLink { OnlineArtistPage(artist: artist) } label: { OnlineArtistRow(artist: artist) }
                    .themedRows()
                }
            } header: {
                SeeAllLink(title: "Artists", more: artists.count > Self.topCollections) { OnlineArtistsPage(artists: artists) }
            }
        }
    }
}

struct SeeAllLink<Page: View>: View {
    let title: String
    let more: Bool
    @ViewBuilder let page: () -> Page

    var body: some View {
        HStack {
            Text(title)
            Spacer()
            if more {
                NavigationLink("See all", destination: page)
                    .font(.subheadline)
                    .textCase(nil)
            }
        }
    }
}

struct OnlineSongsPage: View {
    @State private var actions = OnlineSongActions()
    let songs: [SearchSong]

    var body: some View {
        List {
            DownloadProgress().themedRows()
            OnlineSongsSection(songs: songs, actions: $actions).themedRows()
        }
        .listStyle(.plain)
        .navigationTitle("Songs")
        .onlineSongActions($actions)
    }
}

struct OnlineAlbumsPage: View {
    let albums: [SearchAlbum]

    var body: some View {
        List(albums, id: \.id) { album in
            NavigationLink { OnlineAlbumPage(album: album) } label: { OnlineAlbumRow(album: album) }
                .themedRows()
        }
        .listStyle(.plain)
        .navigationTitle("Albums")
    }
}

struct OnlineArtistsPage: View {
    let artists: [SearchArtist]

    var body: some View {
        List(artists, id: \.id) { artist in
            NavigationLink { OnlineArtistPage(artist: artist) } label: { OnlineArtistRow(artist: artist) }
                .themedRows()
        }
        .listStyle(.plain)
        .navigationTitle("Artists")
    }
}

struct OnlineLinkSections: View {
    let collection: OnlineCollection
    @Binding var actions: OnlineSongActions

    var body: some View {
        let heading = collection.name.isEmpty ? "Shared music" : collection.name
        if !collection.artists.isEmpty {
            Section(heading) {
                ForEach(collection.artists, id: \.id) { artist in
                    NavigationLink { OnlineArtistPage(artist: artist) } label: { OnlineArtistRow(artist: artist) }
                    .themedRows()
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
    @ScaledMetric(relativeTo: .body) private var edge: CGFloat = 44
    let symbol: String
    var circle = false

    var body: some View {
        ZStack {
            Rectangle().fill(.quaternary)
            Image(systemName: symbol).foregroundStyle(.secondary)
        }
        .frame(width: edge, height: edge)
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
                    .disabled(song.held || features.downloads.holds(song))
                Button("Save for later", systemImage: "bookmark") { features.saveWanted([song]) }
            } label: {
                Image(systemName: "ellipsis.circle").font(.title3).touchTarget()
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
    @State private var failed = false
    @State private var readingMore = false

    var body: some View {
        ThemedList {
            if let details { ArtistInformationSection(artist: details) }
            Section("Albums") {
                ForEach(albums, id: \.id) { album in
                    NavigationLink { OnlineAlbumPage(album: album) } label: { OnlineAlbumRow(album: album) }
                    .themedRows()
                }
                if let next {
                    Button(readingMore ? "Reading more releases…" : "More releases") {
                        Task { await readMore(from: next) }
                    }
                    .disabled(readingMore)
                }
                if failed {
                    ReadFailure(text: "The albums could not be read.") { Task { await readArtist() } }
                } else if read, albums.isEmpty {
                    Text("No albums by this artist were found.").foregroundStyle(.secondary)
                }
            }
        }
        .overlay { if !read, !failed { ProgressView() } }
        .navigationTitle(artist.name)
        .navigationBarTitleDisplayMode(.inline)
        .task { await readArtist() }
    }

    private func readArtist() async {
        guard !read else { return }
        failed = false
        let id = artist.id
        let features = library.features

        async let discography = features.onlineAnswer { try $0.artistAlbums(id: id, from: 0) }
        async let information = features.onlineAnswer { try $0.onlineArtist(id: id) }
        let page = await discography
        details = await information ?? nil
        guard let page else {
            failed = true
            return
        }
        failed = false
        albums = page.albums
        next = page.next
        read = true
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
    @State private var failed = false

    var body: some View {
        ThemedList {
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
                if failed {
                    ReadFailure(text: "The releases could not be read.") { Task { await readReleases() } }
                } else if read, releases.isEmpty {
                    Text("No releases of this album were found.").foregroundStyle(.secondary)
                }
            }
        }
        .overlay { if !read, !failed { ProgressView() } }
        .navigationTitle("Album")
        .navigationBarTitleDisplayMode(.inline)
        .task { await readReleases() }
    }

    private func readReleases() async {
        guard !read else { return }
        let group = album.id
        failed = false
        guard let found = await library.features.onlineAnswer({ try $0.onlineReleases(group: group) }) else {
            failed = true
            return
        }
        releases = found
        read = true
    }
}

struct OnlineReleasePage: View {
    @Environment(Library.self) private var library
    let release: SearchRelease
    @State private var songs: [SearchSong] = []
    @State private var read = false
    @State private var failed = false
    @State private var actions = OnlineSongActions()

    var body: some View {
        ThemedList {
            DownloadProgress()
            OnlineSongsSection(songs: songs, heading: release.title, collected: true, actions: $actions)
            if failed {
                ReadFailure(text: "The songs could not be read.") { Task { await readSongs() } }
            } else if read, songs.isEmpty {
                Text("No songs on this release were found.").foregroundStyle(.secondary)
            }
        }
        .overlay { if !read, !failed { ProgressView() } }
        .navigationTitle(release.title)
        .navigationBarTitleDisplayMode(.inline)
        .onlineSongActions($actions)
        .task { await readSongs() }
    }

    private func readSongs() async {
        guard !read else { return }
        let id = release.id
        failed = false
        guard let found = await library.features.onlineAnswer({ try $0.releaseSongs(id: id) }) else {
            failed = true
            return
        }
        songs = found
        read = true
    }
}

extension SearchSong: Identifiable {
    public var id: String { recording.isEmpty ? "\(title)\u{1F}\(artist)\u{1F}\(album)" : recording }
}
