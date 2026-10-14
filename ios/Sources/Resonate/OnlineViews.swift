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
    @State private var downloading: SearchSong?
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
        .sheet(item: $downloading) { DownloadFolderSheet(song: $0) }
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
                            Button("Download") { downloading = song }
                                .disabled(features.downloading)
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

struct DownloadProgress: View {
    @Environment(Library.self) private var library

    var body: some View {
        let features = library.features
        if features.downloading || features.downloadStatus != nil {
            Section {
                if let status = features.downloadStatus { Text(status).font(.footnote) }
                if features.downloading {
                    Text("\(features.downloadedBytes / 1024) KiB received").font(.caption.monospacedDigit())
                    Button("Cancel download", role: .destructive) { features.cancelDownload() }
                }
            }
        }
    }
}

struct DownloadFolderSheet: View {
    @Environment(Library.self) private var library
    @Environment(\.dismiss) private var dismiss
    let song: SearchSong

    var body: some View {
        let destinations = library.folderList.filter { $0.readable && $0.writable }
        NavigationStack {
            List {
                if destinations.isEmpty {
                    Text("Add a music folder with write access first.")
                }
                Section("Choose a music folder") {
                    ForEach(destinations) { folder in
                        Button(folder.name) {
                            library.features.download(song, into: folder)
                            dismiss()
                        }
                    }
                }
            }
            .navigationTitle("Download \(song.title)")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } } }
        }
        .presentationDetents([.medium])
    }
}

struct WantedView: View {
    @Environment(Library.self) private var library
    @State private var missingOnly = true
    @State private var downloading: SearchSong?

    var body: some View {
        let features = library.features
        let shown = features.wanted.filter { !missingOnly || !$0.held }
        List {
            Section {
                Text("Keep recordings you want and download them when your provider has a match.").foregroundStyle(.secondary)
                Toggle("Missing from library only", isOn: $missingOnly)
                Button("Refresh library matches") { features.loadWanted() }
                if !features.online { Text("Enable online features in Settings to download music.") }
            }
            DownloadProgress()
            if shown.isEmpty {
                Text(features.wanted.isEmpty ? "Save a recording from Online search to find it here." : "All saved recordings are in your library.")
                    .foregroundStyle(.secondary)
            }
            ForEach(shown, id: \.id) { wanted in
                VStack(alignment: .leading, spacing: 6) {
                    Text(wanted.song.title).font(.body.weight(.medium))
                    Text("\(wanted.song.artist) · \(wanted.song.album)").font(.subheadline).foregroundStyle(.secondary)
                    if wanted.held {
                        Text("Already in library").foregroundStyle(.tint)
                    } else if features.online {
                        Button("Download") { downloading = wanted.song }
                            .buttonStyle(.glass)
                            .disabled(features.downloading)
                    }
                }
                .swipeActions {
                    Button("Remove", role: .destructive) { features.removeWanted(wanted.id) }
                }
            }
        }
        .navigationTitle("Saved for later")
        .onAppear { features.loadWanted() }
        .sheet(item: $downloading) { DownloadFolderSheet(song: $0) }
    }
}

struct ProviderView: View {
    @Environment(Library.self) private var library
    @Environment(\.openURL) private var openURL
    @State private var account = DownloadAccount(provider: .monochrome, server: "", user: "", password: "", clientId: "", clientSecret: "", refreshToken: "")

    var body: some View {
        let features = library.features
        Form {
            Section {
                Picker("Provider", selection: $account.provider) {
                    ForEach(MusicProvider.every, id: \.self) { Text($0.title).tag($0) }
                }
            } footer: {
                Text("Downloads are matched by recording codes, or by title, artist and duration. Choose the destination folder for each song.")
            }
            if account.provider == .subsonic || account.provider == .monochrome {
                TextField(account.provider == .monochrome ? "Server URL (blank uses the hosted service)" : "Server URL", text: $account.server)
                    .keyboardType(.URL)
                    .textInputAutocapitalization(.never)
            }
            if account.provider == .subsonic {
                TextField("Username", text: $account.user).textInputAutocapitalization(.never)
                SecureField("Password", text: $account.password)
            }
            if account.provider == .tidal {
                Section("TIDAL") {
                    TextField("TIDAL client ID", text: $account.clientId).textInputAutocapitalization(.never)
                    SecureField("Client secret (if required)", text: $account.clientSecret)
                    SecureField("Refresh token", text: $account.refreshToken)
                    Button(features.tidalBusy ? "Waiting for TIDAL…" : "Sign in with TIDAL") { features.authorizeTidal(account) }
                        .disabled(account.clientId.isEmpty || features.tidalBusy)
                    if let request = features.tidalAuthorization {
                        Text("Enter code: \(request.userCode)").font(.title3.weight(.semibold)).textSelection(.enabled)
                        Button("Open TIDAL authorization") {
                            if let url = URL(string: request.verifyAt) { openURL(url) }
                        }
                    }
                }
            }
            Section {
                Button("Save provider") { features.saveProviderAccount(account) }
            } footer: {
                Text("Account credentials are kept in the iOS keychain on this device only.")
            }
        }
        .navigationTitle("Download providers")
        .onAppear { if let saved = features.providerAccount { account = saved } }
        .onChange(of: features.providerAccount) { _, saved in if let saved { account = saved } }
    }
}
