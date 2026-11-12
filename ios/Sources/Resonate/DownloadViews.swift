import ResonateLogic
import ResonateNative
import SwiftUI

struct DownloadsButton: View {
    @Environment(Library.self) private var library

    var body: some View {
        let active = library.features.downloads.activeCount
        NavigationLink {
            DownloadsView().modifier(ThemedPage())
        } label: {
            Label("downloads.title", systemImage: active > 0 ? "arrow.down.circle.fill" : "arrow.down.circle")
        }
        .accessibilityValue(active > 0 ? said("downloads.active", active) : "")
        .simultaneousGesture(TapGesture().onEnded { Keyboard.stow() })
    }
}

struct DownloadProgress: View {
    @Environment(Library.self) private var library

    var body: some View {
        let downloads = library.features.downloads
        DownloadProviderPrompt()
        if !downloads.jobs.isEmpty {
            Section {
                NavigationLink {
                    DownloadsView().modifier(ThemedPage())
                } label: {
                    DownloadSummary(jobs: downloads.jobs, waitsOnTheMeter: downloads.waitsOnTheMeter)
                }
            }
        }
    }
}

struct DownloadProviderPrompt: View {
    @Environment(Library.self) private var library

    var body: some View {
        if library.features.downloads.needsProvider {
            Section {
                NavigationLink {
                    ProviderView().modifier(ThemedPage())
                } label: {
                    Label("downloads.source.set-up", systemImage: "arrow.down.circle")
                }
            } footer: {
                Text("downloads.source.none-ready")
            }
        }
    }
}

struct DownloadSummary: View {
    let jobs: [DownloadJob]
    var waitsOnTheMeter = false

    var body: some View {
        let running = jobs.filter(\.state.inFlight)
        let waiting = jobs.filter { $0.state == .waiting }.count
        let fractions = running.compactMap(\.fraction)
        VStack(alignment: .leading, spacing: 6) {
            Label("downloads.title", systemImage: "arrow.down.circle")
            Text(detail(running: running.count, waiting: waiting))
                .font(.subheadline)
                .foregroundStyle(.secondary)
            if let line = ConnectionMeters.line(waiting: waitsOnTheMeter) {
                Text(line)
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }
            if !running.isEmpty {
                if fractions.isEmpty {
                    ProgressView().progressViewStyle(.linear).accessibilityLabel("downloads.downloading")
                } else {
                    ProgressView(value: fractions.reduce(0, +) / Double(fractions.count)).accessibilityLabel("downloads.downloading")
                }
            }
        }
    }

    private func detail(running: Int, waiting: Int) -> String {
        guard running + waiting == 0 else {
            return [said("downloads.summary.running", running), waiting > 0 ? said("downloads.summary.waiting", waiting) : nil].compactMap { $0 }.joined(separator: " · ")
        }
        let done = jobs.filter { if case .done = $0.state { true } else { false } }.count
        let failed = jobs.filter { if case .failed = $0.state { true } else { false } }.count
        return [said("downloads.summary.done", done), failed > 0 ? said("downloads.summary.failed", failed) : nil].compactMap { $0 }.joined(separator: " · ")
    }
}

struct DownloadsView: View {
    @Environment(Library.self) private var library

    var body: some View {
        let downloads = library.features.downloads
        ThemedList {
            DownloadProviderPrompt()
            if downloads.jobs.isEmpty {
                ContentUnavailableView(
                    "downloads.empty.title",
                    systemImage: "arrow.down.circle",
                    description: Text("downloads.empty.detail")
                )
            }
            if let line = ConnectionMeters.line(waiting: downloads.waitsOnTheMeter) {
                Section {
                    Label(line, systemImage: "antenna.radiowaves.left.and.right")
                        .foregroundStyle(.secondary)
                }
            }
            ForEach(downloads.jobs) { DownloadJobRow(job: $0) }
            if let folder = downloads.rememberedFolder {
                Section {
                    LabeledContent("downloads.folder.saved-to", value: folder.name)
                    Button("downloads.folder.forget") { downloads.forgetFolder() }
                }
            }
        }
        .navigationTitle("downloads.title")
        .toolbar {
            if downloads.jobs.contains(where: \.state.settled) {
                ToolbarItem(placement: .topBarTrailing) {
                    Button("downloads.clear-finished") { downloads.clearSettled() }
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
                Text(job.song.title).font(.body.weight(.medium)).titleLines()
                Text([job.song.artist, job.provider.map { said("downloads.job.from", $0.title) }].compactMap { $0 }.filter { !$0.isEmpty }.joined(separator: " · "))
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .titleLines()
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
            Button("common.cancel", role: .destructive) { downloads.cancel(job.id) }
                .buttonStyle(.bordered)
        case .done(let track):
            HStack {
                Button("common.play") { library.play([track], at: 0) }
                if let albumPage = track.albumPage {
                    Button("downloads.job.go-to-album") { navigation.open(albumPage) }
                }
            }
            .buttonStyle(.bordered)
        case .failed(let reason):
            Text(reason).font(.footnote).foregroundStyle(Color.failure)
            Button("common.try-again") { downloads.retry(job.id) }
                .buttonStyle(.bordered)
        case .cancelled:
            Text("downloads.job.cancelled").font(.footnote).foregroundStyle(.secondary)
            Button("common.try-again") { downloads.retry(job.id) }
                .buttonStyle(.bordered)
        }
    }

    @ViewBuilder
    private var progress: some View {
        switch job.state {
        case .waiting:
            Text("downloads.job.waiting").font(.footnote).foregroundStyle(.secondary)
        case .finding:
            ProgressView().progressViewStyle(.linear).accessibilityLabel("downloads.job.finding")
            Text("downloads.job.finding").font(.footnote).foregroundStyle(.secondary)
        case .validating:
            ProgressView().progressViewStyle(.linear).accessibilityLabel("downloads.job.validating")
            Text("downloads.job.validating").font(.footnote).foregroundStyle(.secondary)
        default:
            if let fraction = job.fraction {
                ProgressView(value: fraction).accessibilityLabel("downloads.job.progress")
                Text(fraction.formatted(.percent.precision(.fractionLength(0))))
                    .font(.footnote.monospacedDigit())
                    .foregroundStyle(.secondary)
            } else {
                ProgressView().progressViewStyle(.linear).accessibilityLabel("downloads.downloading")
                Text(said("downloads.job.received", Int64(clamping: job.received).formatted(.byteCount(style: .file))))
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
        content.sheet(isPresented: $downloads.choosingFolder) { DownloadFolderSheet().sheetSized() }
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
                    Text("downloads.folder.none-writable")
                }
                Section("downloads.folder.choose") {
                    ForEach(destinations) { folder in
                        Button(folder.name) {
                            downloads.choose(folder, remember: remember)
                            dismiss()
                        }
                    }
                }
                Section {
                    Toggle("downloads.folder.remember", isOn: $remember)
                } footer: {
                    Text("downloads.folder.remember.footer")
                }
            }
            .navigationTitle(songs.count == 1 ? said("downloads.folder.title.one", songs[0].title) : said("downloads.folder.title", songs.count))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .cancellationAction) { Button("common.cancel") { dismiss() } } }
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
                Text("wanted.intro").foregroundStyle(.secondary)
                Toggle("wanted.missing-only", isOn: $missingOnly)
                Toggle("wanted.automatic", isOn: Bindable(features.downloads).asksForWants)
                if !features.online {
                    Text("wanted.online-off")
                } else if !missing.isEmpty {
                    Button("wanted.download-all", systemImage: "arrow.down.circle") { features.downloads.enqueue(missing) }
                }
            }
            DownloadProgress()
            if shown.isEmpty {
                Text(said(features.wanted.isEmpty ? "wanted.empty" : "wanted.all-held"))
                    .foregroundStyle(.secondary)
            }
            ForEach(shown, id: \.id) { WantedRow(wanted: $0) }
        }
        .navigationTitle("wanted.title")
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
                Text(song.title).font(.body.weight(.medium)).titleLines()
                Text([song.artist, song.album].filter { !$0.trimmingCharacters(in: .whitespaces).isEmpty }.joined(separator: " · "))
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .titleLines()
                if wanted.held {
                    Label("wanted.held", systemImage: "checkmark.circle.fill")
                        .font(.caption)
                        .foregroundStyle(.tint)
                }
            }
            Spacer(minLength: 0)
            if !wanted.held, features.online {
                Button("wanted.download", systemImage: "arrow.down.circle") { features.downloads.enqueue([song]) }
                    .labelStyle(.iconOnly)
                    .font(.title2)
                    .buttonStyle(.borderless)
                    .disabled(features.downloads.holds(song))
            }
        }
        .swipeActions {
            Button("common.remove", role: .destructive) { features.removeWanted(wanted.id) }
                .tint(.red)
        }
    }
}

func providersDetail(_ asked: [DownloadAccount]) -> String {
    asked.isEmpty ? said("downloads.source.set-up") : asked.map(\.provider.title).joined(separator: " · ")
}

struct ProviderView: View {
    @Environment(Library.self) private var library

    var body: some View {
        let providers = library.features.providers
        ThemedForm {
            Section {
                LabeledContent("providers.asked", value: providers.asked.isEmpty ? said("providers.asked.none") : providersDetail(providers.asked))
            } footer: {
                Text("providers.asked.footer")
            }
            Section {
                Toggle(isOn: Binding(get: { library.features.hostedServices }, set: { library.features.setHostedServices($0) })) {
                    Text("providers.hosted")
                }
            } footer: {
                Text("providers.hosted.footer")
            }
            Section {
                Toggle(isOn: Binding(get: { library.features.stallDownloadsOnMetered }, set: { library.features.setStallDownloadsOnMetered($0) })) {
                    Text("providers.metered")
                }
            } footer: {
                Text("providers.metered.footer")
            }
            ForEach(MusicProvider.every, id: \.self) { provider in
                ProviderSection(saved: providers.account(provider), on: providers.isOn(provider))
            }
        }
        .navigationTitle("providers.title")
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
                Button(said("providers.save", provider.title)) { features.saveProviderAccount(account.trimmed) }
                    .disabled(account.trimmed == saved)
            }
        } footer: {
            Text(provider.purpose)
        }
        .onChange(of: saved) { previous, current in account = followed(previous, current) }
    }

    private func followed(_ previous: DownloadAccount, _ current: DownloadAccount) -> DownloadAccount {
        let unedited = account == previous || account.trimmed == current
        if unedited { return current }
        guard current.holdsAQuickConnectSession, !account.holdsAQuickConnectSession else {
            return account.renewed(current.refreshToken)
        }
        var signed = account
        signed.user = current.user
        signed.password = ""
        signed.token = current.token
        signed.userId = current.userId
        signed.device = current.device
        return signed
    }
}

struct ProviderStatus: View {
    let account: DownloadAccount
    let on: Bool

    var body: some View {
        Text(on ? account.readiness.title(for: account) : said("providers.off"))
            .font(.footnote)
            .foregroundStyle(tint)
    }

    private var tint: Color {
        guard on else { return .secondary }
        return account.readiness == .ready ? Color.confirmation : Color.warning
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
            serverField(said("providers.field.server.hosted"))
        case .hifiApi:
            serverField(said("providers.field.server.hosted"))
            SecureField("providers.field.key", text: $account.password)
        case .subsonic, .jellyfin:
            serverField(said("providers.field.server"))
                .onChange(of: account.server) { _, _ in
                    if account.provider == .jellyfin { account.letGoOfTheQuickConnectSession() }
                }
            TextField("providers.field.username", text: $account.user)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
            SecureField("providers.field.password", text: $account.password)
                .onChange(of: account.password) { _, typed in
                    if !typed.isEmpty { account.letGoOfTheQuickConnectSession() }
                }
            if account.offersQuickConnect {
                Button(said(features.jellyfinBusy ? "providers.jellyfin.waiting" : (account.holdsAQuickConnectSession ? "providers.jellyfin.sign-in-again" : "providers.jellyfin.sign-in"))) {
                    features.quickConnectJellyfin(account.trimmed)
                }
                .disabled(features.jellyfinBusy)
                if features.jellyfinBusy {
                    Button("providers.jellyfin.cancel", role: .cancel) { features.cancelJellyfinQuickConnect() }
                }
                if let code = features.jellyfinCode {
                    Text(said("providers.jellyfin.code", code))
                        .font(.title3.weight(.semibold))
                        .textSelection(.enabled)
                }
            }
        case .tidal:
            TextField("providers.field.tidal-client", text: $account.clientId)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
            SecureField("providers.field.client-secret", text: $account.clientSecret)
            SecureField("providers.field.refresh-token", text: $account.refreshToken)
            Button(said(features.tidalBusy ? "providers.tidal.waiting" : "providers.tidal.sign-in")) { features.authorizeTidal(account.trimmed) }
                .disabled(account.clientId.trimmingCharacters(in: .whitespaces).isEmpty || features.tidalBusy)
            if features.tidalBusy {
                Button("providers.tidal.cancel", role: .cancel) { features.cancelTidalAuthorization() }
            }
            if let request = features.tidalAuthorization {
                Text(said("providers.tidal.code", request.userCode)).font(.title3.weight(.semibold)).textSelection(.enabled)
                Button("providers.tidal.open") {
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
            said(account.usesItsOwnServer ? "providers.ready.own" : "providers.ready.hosted")
        case .ready: said("providers.ready")
        case .missingServer: said("providers.missing.server")
        case .missingLogin: said("providers.missing.login")
        case .missingClient: said("providers.missing.client")
        case .needsSignIn: said("providers.missing.sign-in")
        }
    }
}

extension MusicProvider {
    var purpose: String {
        switch self {
        case .subsonic: said("providers.purpose.subsonic")
        case .jellyfin: said("providers.purpose.jellyfin")
        case .tidal: said("providers.purpose.tidal")
        case .hifiApi: said("providers.purpose.hifi-api")
        case .monochrome: said("providers.purpose.monochrome")
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
