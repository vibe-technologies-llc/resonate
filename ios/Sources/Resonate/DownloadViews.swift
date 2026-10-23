import ResonateLogic
import ResonateNative
import SwiftUI

struct DownloadProgress: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation

    var body: some View {
        let downloads = library.features.downloads
        if downloads.needsProvider {
            Section {
                Text("No download provider is ready. Switch one on and fill in what it needs.")
                Button("Set up a download provider") {
                    downloads.missedProvider = false
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
        ThemedList {
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
    @ScaledMetric(relativeTo: .body) private var artworkEdge: CGFloat = 44
    let job: DownloadJob

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            Group {
                if case .done(let track) = job.state {
                    ArtworkView(track: track, corner: 8)
                } else {
                    OnlinePlaceholder(symbol: "arrow.down.circle")
                }
            }
            .frame(width: artworkEdge, height: artworkEdge)
            VStack(alignment: .leading, spacing: 6) {
                Text(job.song.title).font(.body.weight(.medium)).lineLimit(1)
                Text([job.song.artist, job.provider.map { "from \($0.title)" }].compactMap { $0 }.filter { !$0.isEmpty }.joined(separator: " · "))
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                state
            }
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
                if let albumPage = track.albumPage {
                    Button("Go to album") { navigation.open(albumPage) }
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
        ThemedList {
            Section {
                Text("Keep recordings you want and download them when one of your providers has a match.").foregroundStyle(.secondary)
                Toggle("Missing from library only", isOn: $missingOnly)
                Toggle("Download saved songs automatically", isOn: Bindable(features.downloads).asksForWants)
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
    @ScaledMetric(relativeTo: .body) private var artworkEdge: CGFloat = 44
    let wanted: WantedSong

    var body: some View {
        let features = library.features
        let song = wanted.song
        HStack(spacing: 12) {
            OnlinePlaceholder(symbol: wanted.held ? "checkmark" : "music.note")
                .frame(width: artworkEdge, height: artworkEdge)
            VStack(alignment: .leading, spacing: 2) {
                Text(song.title).font(.body.weight(.medium)).lineLimit(1)
                Text([song.artist, song.album].filter { !$0.trimmingCharacters(in: .whitespaces).isEmpty }.joined(separator: " · "))
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                if wanted.held {
                    Label("In your library", systemImage: "checkmark.circle.fill")
                        .font(.caption)
                        .foregroundStyle(.tint)
                }
            }
            Spacer(minLength: 0)
            if !wanted.held, features.online {
                Button("Download", systemImage: "arrow.down.circle") { features.downloads.enqueue([song]) }
                    .labelStyle(.iconOnly)
                    .font(.title2)
                    .buttonStyle(.borderless)
                    .disabled(features.downloads.holds(song))
            }
        }
        .swipeActions {
            Button("Remove", role: .destructive) { features.removeWanted(wanted.id) }
                .tint(.red)
        }
    }
}

func providersDetail(_ asked: [DownloadAccount]) -> String {
    asked.isEmpty ? "Set up a download provider" : asked.map(\.provider.title).joined(separator: " · ")
}

struct ProviderView: View {
    @Environment(Library.self) private var library

    var body: some View {
        let providers = library.features.providers
        ThemedForm {
            Section {
                LabeledContent("Asked for each song", value: providers.asked.isEmpty ? "None ready" : providersDetail(providers.asked))
            } footer: {
                Text("Every provider that is on and ready is asked, in the order below: your own server, then your TIDAL subscription, then the hosted services. An earlier provider's match is taken over a later one's when it answers within a few seconds. Songs are matched by recording codes, or by title, artist and duration. Credentials are kept in the iOS keychain on this device only.")
            }
            Section {
                Toggle(isOn: Binding(get: { library.features.hostedServices }, set: { library.features.setHostedServices($0) })) {
                    Text("Ask the hosted services")
                }
            } footer: {
                Text("A Monochrome or hifi-api provider with no server address is asked at the hosted service unless this is off.")
            }
            ForEach(MusicProvider.every, id: \.self) { provider in
                ProviderSection(saved: providers.account(provider), on: providers.isOn(provider))
            }
        }
        .navigationTitle("Download providers")
    }
}

struct ProviderSection: View {
    @Environment(Library.self) private var library
    let saved: DownloadAccount
    let on: Bool
    @State private var account: DownloadAccount

    init(saved: DownloadAccount, on: Bool) {
        self.saved = saved
        self.on = on
        _account = State(initialValue: saved)
    }

    var body: some View {
        let features = library.features
        let provider = saved.provider
        Section {
            Toggle(isOn: Binding(get: { on }, set: { features.turnProvider(provider, on: $0) })) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(provider.title)
                    ProviderStatus(account: saved, on: on)
                }
            }
            if on {
                ProviderFields(account: $account)
                Button("Save \(provider.title)") { features.saveProviderAccount(account.trimmed) }
                    .disabled(account.trimmed == saved)
            }
        } footer: {
            Text(provider.purpose)
        }
        .onChange(of: saved) { previous, current in account = followed(previous, current) }
    }

    private func followed(_ previous: DownloadAccount, _ current: DownloadAccount) -> DownloadAccount {
        let unedited = account == previous || account.trimmed == current
        return unedited ? current : account.renewed(current.refreshToken)
    }
}

struct ProviderStatus: View {
    let account: DownloadAccount
    let on: Bool

    var body: some View {
        Text(on ? account.readiness.title(for: account) : "Off")
            .font(.footnote)
            .foregroundStyle(tint)
    }

    private var tint: Color {
        guard on else { return .secondary }
        return account.readiness == .ready ? .green : .orange
    }
}

struct ProviderFields: View {
    @Environment(Library.self) private var library
    @Environment(\.openURL) private var openURL
    @Binding var account: DownloadAccount

    var body: some View {
        let features = library.features
        switch account.provider {
        case .monochrome:
            serverField("Server URL (blank uses the hosted service)")
        case .hifiApi:
            serverField("Server URL (blank uses the hosted service)")
            SecureField("Key for your own server (optional)", text: $account.password)
        case .subsonic:
            serverField("Server URL")
            TextField("Username", text: $account.user)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
            SecureField("Password", text: $account.password)
        case .tidal:
            TextField("TIDAL client ID", text: $account.clientId)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
            SecureField("Client secret (if required)", text: $account.clientSecret)
            SecureField("Refresh token", text: $account.refreshToken)
            Button(features.tidalBusy ? "Waiting for TIDAL…" : "Sign in with TIDAL") { features.authorizeTidal(account.trimmed) }
                .disabled(account.clientId.trimmingCharacters(in: .whitespaces).isEmpty || features.tidalBusy)
            if features.tidalBusy {
                Button("Cancel sign-in", role: .cancel) { features.cancelTidalAuthorization() }
            }
            if let request = features.tidalAuthorization {
                Text("Enter code: \(request.userCode)").font(.title3.weight(.semibold)).textSelection(.enabled)
                Button("Open TIDAL authorization") {
                    if let url = URL(string: request.verifyAt) { openURL(url) }
                }
            }
        }
    }

    private func serverField(_ title: String) -> some View {
        TextField(title, text: $account.server)
            .keyboardType(.URL)
            .textInputAutocapitalization(.never)
            .autocorrectionDisabled()
    }
}

extension ProviderReadiness {
    func title(for account: DownloadAccount) -> String {
        switch self {
        case .ready where account.provider == .monochrome || account.provider == .hifiApi:
            account.usesItsOwnServer ? "Ready · your server" : "Ready · hosted service"
        case .ready: "Ready"
        case .missingServer: "Missing server"
        case .missingLogin: "Missing username or password"
        case .missingClient: "Missing client ID"
        case .needsSignIn: "Needs sign-in"
        }
    }
}

extension MusicProvider {
    var purpose: String {
        switch self {
        case .subsonic: "Your own Subsonic, Navidrome or compatible server, sending its original files."
        case .tidal: "Your TIDAL subscription, lossless only, signed in with your own client ID."
        case .hifiApi: "TIDAL through a hifi-api server."
        case .monochrome: "The track streamer behind Monochrome."
        }
    }
}

extension DownloadAccount {
    var trimmed: DownloadAccount {
        var trimmed = self
        trimmed.server = server.trimmingCharacters(in: .whitespacesAndNewlines)
        trimmed.user = user.trimmingCharacters(in: .whitespacesAndNewlines)
        trimmed.clientId = clientId.trimmingCharacters(in: .whitespacesAndNewlines)
        trimmed.refreshToken = refreshToken.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed
    }

    func renewed(_ refreshToken: String) -> DownloadAccount {
        var renewed = self
        renewed.refreshToken = refreshToken
        return renewed
    }
}
