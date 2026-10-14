import ResonateNative
import SwiftUI

enum OnlineMode: String, CaseIterable, Identifiable {
    case songs = "Songs"
    case albums = "Albums"
    case artists = "Artists"
    case link = "Link"
    case releases = "Releases"
    case albumSongs = "Album songs"

    var id: String { rawValue }
    static let asked: [OnlineMode] = [.songs, .albums, .artists, .link]
}

struct OnlineView: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    @State private var mode = OnlineMode.songs
    @State private var query = ""
    @State private var heading = ""
    @State private var discography: String?
    @State private var importing = false
    @State private var importName = ""

    var body: some View {
        let features = library.features
        List {
            if !features.online {
                Section {
                    Text("Online features are disabled. Turn them on in Settings to search and download music.")
                    Button("Open Settings") { navigation.tab = .settings }
                }
            } else {
                Section {
                    Picker("Search for", selection: $mode) {
                        ForEach(OnlineMode.asked) { Text($0.rawValue).tag($0) }
                    }
                    .pickerStyle(.segmented)
                    TextField(mode == .link ? "Album, artist, song or playlist link" : "Song, album or artist", text: $query)
                        .textInputAutocapitalization(.never)
                        .onSubmit(search)
                    Button("Search online", action: search)
                        .disabled(query.trimmingCharacters(in: .whitespaces).isEmpty || features.searching)
                }
                DownloadProgress()
                if features.searching { ProgressView().frame(maxWidth: .infinity) }
                if let artist = features.artist { ArtistInformationSection(artist: artist) }
                results(features)
            }
        }
        .navigationTitle("Online music search")
        .onChange(of: features.onlineCollection) { _, opened in
            guard let opened else { return }
            heading = opened.name.isEmpty ? "Shared music" : opened.name
            mode = !opened.releases.isEmpty ? .releases : !opened.artists.isEmpty ? .artists : .albumSongs
        }
        .downloadDestinationPrompt()
        .alert("Import matching local songs", isPresented: $importing) {
            TextField("Playlist name", text: $importName)
            Button("Cancel", role: .cancel) {}
            Button("Import") { features.importPlaylist(importName.isEmpty ? heading : importName, features.onlineSongs) }
        } message: {
            Text("Songs already in your library are added in the shared order. Save the remaining recordings for later to download them.")
        }
    }

    private func search() {
        let text = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else { return }
        let features = library.features
        switch mode {
        case .link: features.openLink(text)
        case .albums: features.searchAlbums(text)
        case .artists: features.searchArtists(text)
        default:
            mode = .songs
            features.searchOnline(text)
        }
    }

    @ViewBuilder
    private func results(_ features: Features) -> some View {
        switch mode {
        case .albums:
            Section {
                ForEach(features.onlineAlbums, id: \.id) { album in
                    Button {
                        heading = album.title
                        mode = .releases
                        features.loadReleases(album.id)
                    } label: {
                        VStack(alignment: .leading) {
                            Text(album.title).font(.body.weight(.medium))
                            if !album.artist.isEmpty { Text(album.artist) }
                            Text(album.date).font(.caption).foregroundStyle(.secondary)
                        }
                    }
                }
                if let next = features.artistAlbumsNext, let discography {
                    Button("More releases") { features.artistAlbums(discography, from: next) }
                }
            }
        case .artists:
            Section {
                ForEach(features.onlineArtists, id: \.id) { candidate in
                    VStack(alignment: .leading, spacing: 6) {
                        Text(candidate.name).font(.body.weight(.medium))
                        Text(candidate.kind).font(.caption).foregroundStyle(.secondary)
                        HStack {
                            Button("Artist details") { features.onlineArtist(candidate.id) }
                            Button("Find albums") {
                                discography = candidate.id
                                mode = .albums
                                features.artistAlbums(candidate.id)
                            }
                        }
                        .buttonStyle(.glass)
                    }
                }
            }
        case .releases:
            Section(heading) {
                ForEach(features.onlineReleases, id: \.id) { release in
                    Button {
                        heading = release.title
                        mode = .albumSongs
                        features.releaseSongs(release.id)
                    } label: {
                        VStack(alignment: .leading) {
                            Text(release.title).font(.body.weight(.medium))
                            Text([release.date, release.country, release.songs > 0 ? counted(Int(release.songs), "song") : "View track list"]
                                .filter { !$0.isEmpty }.joined(separator: " · "))
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                    }
                }
            }
        case .songs, .link, .albumSongs:
            Section {
                if mode == .albumSongs, !features.onlineSongs.isEmpty {
                    Button("Save all for later") { features.saveWanted(features.onlineSongs) }
                    Button("Import local matches as playlist") {
                        importName = heading
                        importing = true
                    }
                }
                ForEach(features.onlineSongs, id: \.self) { song in
                    VStack(alignment: .leading, spacing: 6) {
                        Text(song.title).font(.body.weight(.medium))
                        Text(song.artist)
                        Text(song.album).font(.caption).foregroundStyle(.secondary)
                        if let duration = song.durationMs { Text(clock(duration)).font(.caption).foregroundStyle(.secondary) }
                        HStack {
                            Button("Download") { features.downloads.enqueue([song]) }
                                .disabled(features.downloads.holds(song))
                            Button("Save for later") { features.saveWanted([song]) }
                        }
                        .buttonStyle(.glass)
                    }
                }
            } header: {
                if mode == .albumSongs { Text(heading) }
            }
        }
    }
}

extension SearchSong: Identifiable {
    public var id: String { recording.isEmpty ? "\(title)\u{1F}\(artist)\u{1F}\(album)" : recording }
}
