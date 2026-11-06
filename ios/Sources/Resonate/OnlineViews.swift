import ResonateLogic
import ResonateNative
import SwiftUI

struct OnlineSearchContent: View {
    @Environment(Library.self) private var library
    @Binding var actions: OnlineSongActions

    var body: some View {
        let features = library.features
        let text = features.onlineQuery.trimmingCharacters(in: .whitespacesAndNewlines)
        let found = features.onlineFound
        let current = found.text == text

        if text.isEmpty {
            ContentUnavailableView(
                "Search online",
                systemImage: "globe",
                description: Text("Find songs, artists, albums and shared music. Paste a shared link to open it.")
            )
            .listRowSeparator(.hidden)
        } else if let service = profileLinkService(text: text) {
            OnlineProfileSection(link: text, service: service)
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

struct OnlineProfileSection: View {
    @Environment(Library.self) private var library
    let link: String
    let service: String

    var body: some View {
        let features = library.features
        Section {
            Button(FollowedProfileWords.follow(service), systemImage: "person.crop.circle.badge.checkmark") {
                Keyboard.stow()
                features.followProfile(link, keptInStep: true)
            }
            Button(FollowedProfileWords.moveOnce, systemImage: "square.and.arrow.down") {
                Keyboard.stow()
                features.followProfile(link, keptInStep: false)
            }
            if features.followingProfile {
                HStack(spacing: 10) {
                    ProgressView().accessibilityHidden(true)
                    Text(FollowedProfileWords.reading).foregroundStyle(.secondary)
                }
                .font(.subheadline)
                .accessibilityElement(children: .combine)
            }
        } header: {
            Text(FollowedProfileWords.heading(service))
        } footer: {
            Text(FollowedProfileWords.explained(service))
        }
        .disabled(features.followingProfile)
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
        .announced(link ? "No music could be read from this link" : "No results for \(text)")
    }
}

struct OnlinePending: View {
    let text: String
    let awaited: OnlineMode?

    var body: some View {
        HStack(spacing: 10) {
            ProgressView().accessibilityHidden(true)
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
            OnlineLinkSections(collection: link, link: found.text, actions: $actions)
        }
        if let songs = found.songs, !songs.isEmpty {
            let shown = Array(songs.prefix(Self.topSongs))
            Section {
                ForEach(shown.indices, id: \.self) { index in
                    OnlineSongRow(song: shown[index])
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
                NavigationLink(destination: page) {
                    Text("See all").touchTarget()
                }
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
    let link: String
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
            OnlineSongsSection(songs: collection.songs, heading: heading, collected: true, link: link, actions: $actions)
        }
    }
}

struct OnlineSongActions {
    var importing = false
    var importName = ""
    var importedSongs: [SearchSong] = []
    var importedLink: ImportedLink?
}

struct ImportedLink {
    let link: String
    let service: String
    let keptInStep: Bool
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
            .alert(title, isPresented: $actions.importing) {
                TextField("Playlist name", text: $actions.importName)
                Button("Cancel", role: .cancel) {}
                Button("Import") {
                    let typed = actions.importName.trimmingCharacters(in: .whitespacesAndNewlines)
                    let name = typed.isEmpty ? "Shared music" : typed
                    if let linked = actions.importedLink {
                        library.features.importLinkedPlaylist(
                            link: linked.link, name: name, songs: actions.importedSongs, keptInStep: linked.keptInStep)
                    } else {
                        library.features.importPlaylist(name, actions.importedSongs)
                    }
                }
            } message: {
                Text(message)
            }
    }

    private var title: String {
        guard let linked = actions.importedLink, linked.keptInStep else { return "Import matching local songs" }
        return LinkedPlaylistWords.keepInStep(linked.service)
    }

    private var message: String {
        guard let linked = actions.importedLink, linked.keptInStep else {
            return "Songs already in your library are added in the shared order. Save the remaining recordings for later to download them."
        }
        return LinkedPlaylistWords.keepingExplained(linked.service)
    }
}

struct OnlineSongsSection: View {
    @Environment(Library.self) private var library
    let songs: [SearchSong]
    var heading = ""
    var collected = false
    var link: String?
    @Binding var actions: OnlineSongActions

    var body: some View {
        let features = library.features
        Section {
            if collected, !songs.isEmpty {
                Button("Save all for later", systemImage: "bookmark") { features.saveWanted(songs) }
                if let link, let service = playlistLinkService(text: link) {
                    Menu {
                        Button("Import once", systemImage: "square.and.arrow.down") {
                            importing(ImportedLink(link: link, service: service, keptInStep: false))
                        }
                        Button(LinkedPlaylistWords.keepInStep(service), systemImage: "arrow.triangle.2.circlepath") {
                            importing(ImportedLink(link: link, service: service, keptInStep: true))
                        }
                    } label: {
                        Label("Import as playlist", systemImage: "text.badge.plus")
                    }
                } else {
                    Button("Import local matches as playlist", systemImage: "text.badge.plus") { importing(nil) }
                }
            }
            ForEach(songs.indices, id: \.self) { index in
                OnlineSongRow(song: songs[index])
            }
        } header: {
            if !heading.isEmpty { Text(heading) }
        }
    }

    private func importing(_ linked: ImportedLink?) {
        actions.importName = heading
        actions.importedSongs = songs
        actions.importedLink = linked
        actions.importing = true
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
        .modifier(ArtworkClip(circle: circle, corner: 8))
        .accessibilityHidden(true)
    }
}

struct OnlineSongRow: View {
    @Environment(Library.self) private var library
    @ScaledMetric(relativeTo: .body) private var artworkEdge: CGFloat = 44
    let song: SearchSong
    @State private var heldTrack: Track?
    @State private var heldSong: String?

    var body: some View {
        let features = library.features
        let downloads = features.downloads
        let track = (heldSong == song.id ? heldTrack : nil) ?? downloads.finishedTrack(for: song)
        let standing: DownloadStanding = track == nil ? downloads.standing(of: song) : .held
        HStack(spacing: 12) {
            Button {
                Keyboard.stow()
                if let track {
                    library.play([track], at: 0)
                } else if standing.startsOnTap {
                    downloads.enqueue([song])
                }
            } label: {
                HStack(spacing: 12) {
                    if let track {
                        ArtworkView(track: track, corner: 8).frame(width: artworkEdge, height: artworkEdge)
                    } else {
                        OnlinePlaceholder(symbol: "music.note")
                    }
                    VStack(alignment: .leading, spacing: 2) {
                        Text(song.title).font(.body.weight(.medium)).titleLines()
                        Text([song.artist, song.album].filter { !$0.isEmpty }.joined(separator: " · "))
                            .font(.subheadline)
                            .foregroundStyle(.secondary)
                            .titleLines()
                        OnlineSongCaption(song: song, standing: standing)
                    }
                    Spacer(minLength: 0)
                }
                .contentShape(.rect)
            }
            .buttonStyle(.plain)
            .accessibilityHint(track != nil ? "Plays the song" : standing.startsOnTap ? "Downloads the song" : "")
            DownloadBadge(standing: standing) { downloads.cancel(song) }
        }
        .task(id: "\(song.id):\(library.catalogEpoch)") {
            guard let native = try? await library.player() else { return }
            let matched = (try? await Work.onScreen { try native.matchLocalSong(song: song) }) ?? nil
            guard !Task.isCancelled else { return }
            heldTrack = matched
            heldSong = song.id
        }
        .swipeActions(edge: .trailing) {
            Button("Save for later", systemImage: "bookmark") { features.saveWanted([song]) }
                .tint(.indigo)
        }
        .contextMenu {
            Button("Download", systemImage: "arrow.down.circle") { downloads.enqueue([song]) }
                .disabled(!standing.startsOnTap)
            Button("Save for later", systemImage: "bookmark") { features.saveWanted([song]) }
        }
        .accessibilityElement(children: .contain)
        .accessibilityActions {
            if standing.cancellable {
                Button("Cancel download") { downloads.cancel(song) }
            }
        }
        .sensoryFeedback(.success, trigger: standing == .held)
    }
}

struct OnlineSongCaption: View {
    let song: SearchSong
    let standing: DownloadStanding

    var body: some View {
        HStack(spacing: 6) {
            if let duration = song.durationMs {
                Text(clock(duration)).font(.caption.monospacedDigit()).foregroundStyle(.secondary)
            }
            switch standing {
            case .available:
                EmptyView()
            case .waiting:
                Text("Waiting").font(.caption).foregroundStyle(.secondary)
            case .running(let fraction):
                if let fraction {
                    Text(fraction.formatted(.percent.precision(.fractionLength(0))))
                        .font(.caption.monospacedDigit())
                        .foregroundStyle(.secondary)
                } else {
                    Text("Finding a match").font(.caption).foregroundStyle(.secondary)
                }
            case .held:
                Label("In your library", systemImage: "checkmark.circle.fill")
                    .font(.caption)
                    .foregroundStyle(.tint)
            case .failed(let reason):
                Text(reason).font(.caption).foregroundStyle(Color.failure).titleLines()
            case .cancelled:
                Text("Cancelled").font(.caption).foregroundStyle(.secondary)
            }
        }
    }
}

struct DownloadBadge: View {
    let standing: DownloadStanding
    let stop: () -> Void

    var body: some View {
        switch standing {
        case .available:
            glyph("arrow.down.circle").foregroundStyle(.tint)
        case .waiting:
            Button(action: stop) {
                ProgressView().accessibilityHidden(true).touchTarget().buttonShape()
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Cancel download")
        case .running(let fraction):
            Button(action: stop) {
                Group {
                    if let fraction {
                        Circle()
                            .stroke(.secondary.opacity(0.2), lineWidth: 2)
                            .overlay {
                                Circle()
                                    .trim(from: 0, to: fraction)
                                    .stroke(.tint, style: StrokeStyle(lineWidth: 2, lineCap: .round))
                                    .rotationEffect(.degrees(-90))
                            }
                    } else {
                        ProgressView().accessibilityHidden(true)
                    }
                }
                .frame(width: 24, height: 24)
                .touchTarget()
                .buttonShape()
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Cancel download")
        case .held:
            glyph("checkmark.circle.fill").foregroundStyle(.tint)
        case .failed, .cancelled:
            glyph("arrow.clockwise.circle").foregroundStyle(Color.failure)
        }
    }

    private func glyph(_ name: String) -> some View {
        Image(systemName: name).font(.title2).touchTarget().accessibilityHidden(true)
    }
}

struct OnlineAlbumRow: View {
    let album: SearchAlbum

    var body: some View {
        HStack(spacing: 12) {
            OnlinePlaceholder(symbol: "square.stack")
            VStack(alignment: .leading, spacing: 2) {
                Text(album.title).font(.body.weight(.medium)).titleLines()
                Text([album.artist, album.date].filter { !$0.isEmpty }.joined(separator: " · "))
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .titleLines()
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
                Text(artist.name).font(.body.weight(.medium)).titleLines()
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
                Text(release.title).font(.body.weight(.medium)).titleLines()
                Text([release.date, release.country, release.songs > 0 ? counted(Int(release.songs), "song") : "View track list"]
                    .filter { !$0.isEmpty }
                    .joined(separator: " · "))
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .titleLines()
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
        .overlay { if !read, !failed { ProgressView().accessibilityLabel("Loading") } }
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
        .overlay { if !read, !failed { ProgressView().accessibilityLabel("Loading") } }
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
        .overlay { if !read, !failed { ProgressView().accessibilityLabel("Loading") } }
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
