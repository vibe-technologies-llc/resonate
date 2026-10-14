import ResonateNative
import SwiftUI

struct DownloadProgress: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation

    var body: some View {
        let downloads = library.features.downloads
        if downloads.needsProvider {
            Section {
                Text("Set up a download provider to download music.")
                Button("Set up a download provider") {
                    downloads.needsProvider = false
                    navigation.open(.providers)
                }
            }
        }
        if !downloads.jobs.isEmpty {
            Section {
                NavigationLink {
                    DownloadsView()
                } label: {
                    DownloadSummary(jobs: downloads.jobs)
                }
            }
        }
    }
}

struct DownloadSummary: View {
    let jobs: [DownloadJob]

    var body: some View {
        let running = jobs.filter(\.state.inFlight)
        let waiting = jobs.filter { $0.state == .waiting }.count
        let fractions = running.compactMap(\.fraction)
        VStack(alignment: .leading, spacing: 6) {
            Label("Downloads", systemImage: "arrow.down.circle")
            Text(detail(running: running.count, waiting: waiting))
                .font(.subheadline)
                .foregroundStyle(.secondary)
            if !running.isEmpty {
                if fractions.isEmpty {
                    ProgressView().progressViewStyle(.linear)
                } else {
                    ProgressView(value: fractions.reduce(0, +) / Double(fractions.count))
                }
            }
        }
    }

    private func detail(running: Int, waiting: Int) -> String {
        guard running + waiting == 0 else {
            return ["\(running) downloading", waiting > 0 ? "\(waiting) waiting" : nil].compactMap { $0 }.joined(separator: " · ")
        }
        let done = jobs.filter { if case .done = $0.state { true } else { false } }.count
        let failed = jobs.filter { if case .failed = $0.state { true } else { false } }.count
        return ["\(done) downloaded", failed > 0 ? "\(failed) failed" : nil].compactMap { $0 }.joined(separator: " · ")
    }
}

struct DownloadsView: View {
    @Environment(Library.self) private var library

    var body: some View {
        let downloads = library.features.downloads
        List {
            if downloads.jobs.isEmpty {
                ContentUnavailableView(
                    "No downloads",
                    systemImage: "arrow.down.circle",
                    description: Text("Songs you download from Online search or Saved for later appear here.")
                )
            }
            ForEach(downloads.jobs) { DownloadJobRow(job: $0) }
            if let folder = downloads.rememberedFolder {
                Section {
                    LabeledContent("Saved to", value: folder.name)
                    Button("Ask for a folder each time") { downloads.forgetFolder() }
                }
            }
        }
        .navigationTitle("Downloads")
        .toolbar {
            if downloads.jobs.contains(where: \.state.settled) {
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Clear finished") { downloads.clearSettled() }
                }
            }
        }
    }
}

struct DownloadJobRow: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    let job: DownloadJob

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(job.song.title).font(.body.weight(.medium)).lineLimit(1)
            Text([job.song.artist, job.provider?.title].compactMap { $0 }.filter { !$0.isEmpty }.joined(separator: " · "))
                .font(.subheadline)
                .foregroundStyle(.secondary)
                .lineLimit(1)
            state
        }
    }

    @ViewBuilder
    private var state: some View {
        let downloads = library.features.downloads
        switch job.state {
        case .waiting, .finding, .downloading, .validating:
            progress
            Button("Cancel", role: .destructive) { downloads.cancel(job.id) }
                .buttonStyle(.glass)
        case .done(let track):
            HStack {
                Button("Play") { library.play([track], at: 0) }
                if !track.album.isEmpty {
                    Button("Go to album") { navigation.open(.album(title: track.album, artist: track.groupingArtist)) }
                }
            }
            .buttonStyle(.glass)
        case .failed(let reason):
            Text(reason).font(.footnote).foregroundStyle(.red)
            Button("Try again") { downloads.retry(job.id) }
                .buttonStyle(.glass)
        case .cancelled:
            Text("Cancelled").font(.footnote).foregroundStyle(.secondary)
            Button("Try again") { downloads.retry(job.id) }
                .buttonStyle(.glass)
        }
    }

    @ViewBuilder
    private var progress: some View {
        switch job.state {
        case .waiting:
            Text("Waiting").font(.footnote).foregroundStyle(.secondary)
        case .finding:
            ProgressView().progressViewStyle(.linear)
            Text("Finding a match").font(.footnote).foregroundStyle(.secondary)
        case .validating:
            ProgressView().progressViewStyle(.linear)
            Text("Checking the download").font(.footnote).foregroundStyle(.secondary)
        default:
            if let fraction = job.fraction {
                ProgressView(value: fraction)
                Text(fraction.formatted(.percent.precision(.fractionLength(0))))
                    .font(.footnote.monospacedDigit())
                    .foregroundStyle(.secondary)
            } else {
                ProgressView().progressViewStyle(.linear)
                Text("\(ByteCountFormatter.string(fromByteCount: Int64(clamping: job.received), countStyle: .file)) received")
                    .font(.footnote.monospacedDigit())
                    .foregroundStyle(.secondary)
            }
        }
    }
}

struct DownloadDestinationPrompt: ViewModifier {
    @Environment(Library.self) private var library

    func body(content: Content) -> some View {
        @Bindable var downloads = library.features.downloads
        content.sheet(isPresented: $downloads.choosingFolder) { DownloadFolderSheet() }
    }
}

extension View {
    func downloadDestinationPrompt() -> some View {
        modifier(DownloadDestinationPrompt())
    }
}

struct DownloadFolderSheet: View {
    @Environment(Library.self) private var library
    @Environment(\.dismiss) private var dismiss
    @State private var remember = true

    var body: some View {
        let downloads = library.features.downloads
        let songs = downloads.awaitingFolder
        let destinations = library.folderList.filter { $0.readable && $0.writable }
        NavigationStack {
            List {
                if destinations.isEmpty {
                    Text("Add a music folder with write access first.")
                }
                Section("Choose a music folder") {
                    ForEach(destinations) { folder in
                        Button(folder.name) {
                            downloads.choose(folder, remember: remember)
                            dismiss()
                        }
                    }
                }
                Section {
                    Toggle("Remember this folder", isOn: $remember)
                } footer: {
                    Text("Later downloads go straight to this folder. Downloads lets you choose again.")
                }
            }
            .navigationTitle(songs.count == 1 ? "Download \(songs[0].title)" : "Download \(counted(songs.count, "song"))")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } } }
        }
        .presentationDetents([.medium, .large])
    }
}

struct WantedView: View {
    @Environment(Library.self) private var library
    @State private var missingOnly = true

    var body: some View {
        let features = library.features
        let shown = features.wanted.filter { !missingOnly || !$0.held }
        let missing = features.wanted.filter { !$0.held }.map(\.song)
        List {
            Section {
                Text("Keep recordings you want and download them when your provider has a match.").foregroundStyle(.secondary)
                Toggle("Missing from library only", isOn: $missingOnly)
                if !features.online {
                    Text("Enable online features in Settings to download music.")
                } else if !missing.isEmpty {
                    Button("Download all missing", systemImage: "arrow.down.circle") { features.downloads.enqueue(missing) }
                }
            }
            DownloadProgress()
            if shown.isEmpty {
                Text(features.wanted.isEmpty ? "Save a recording from Online search to find it here." : "All saved recordings are in your library.")
                    .foregroundStyle(.secondary)
            }
            ForEach(shown, id: \.id) { WantedRow(wanted: $0) }
        }
        .navigationTitle("Saved for later")
        .refreshable { await features.rereadWanted() }
        .onAppear { features.loadWanted() }
        .downloadDestinationPrompt()
    }
}

struct WantedRow: View {
    @Environment(Library.self) private var library
    let wanted: WantedSong

    var body: some View {
        let features = library.features
        let song = wanted.song
        VStack(alignment: .leading, spacing: 6) {
            Text(song.title).font(.body.weight(.medium))
            Text(song.artist).font(.subheadline).foregroundStyle(.secondary)
            if !song.album.trimmingCharacters(in: .whitespaces).isEmpty {
                Text(song.album).font(.caption).foregroundStyle(.secondary)
            }
            if wanted.held {
                Label("In your library", systemImage: "checkmark.circle.fill")
                    .font(.subheadline)
                    .foregroundStyle(.tint)
            }
            HStack {
                if !wanted.held, features.online {
                    Button("Download") { features.downloads.enqueue([song]) }
                        .disabled(features.downloads.holds(song))
                }
                Button("Remove", role: .destructive) { features.removeWanted(wanted.id) }
            }
            .buttonStyle(.glass)
        }
        .swipeActions {
            Button("Remove", role: .destructive) { features.removeWanted(wanted.id) }
        }
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
