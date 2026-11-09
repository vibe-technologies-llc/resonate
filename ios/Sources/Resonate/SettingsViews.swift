import AVFoundation
import ResonateLogic
import ResonateNative
import SwiftUI
import UniformTypeIdentifiers

struct SettingsView: View {
    @Environment(Library.self) private var library
    @State private var search = ""
    @State private var confirmingErase = false

    var body: some View {
        let searching = !search.trimmingCharacters(in: .whitespaces).isEmpty
        let matches = SettingsSearch.matching(search, hiding: hidden)
        ThemedList {
            ForEach(SettingsGroup.allCases) { group in
                let rows = matches.filter { $0.place.group == group }
                if !rows.isEmpty {
                    Section {
                        ForEach(rows) { match in
                            SettingsEntry(match: match, searching: searching) { confirmingErase = true }
                        }
                    } header: {
                        Text(group.title)
                    } footer: {
                        if group == .app, !searching {
                            Text("Erasing removes your library, playlists, favourites, history, settings, accounts and pairing code from this iPhone and deletes every song Resonate lists. Your other devices are not touched.")
                        }
                    }
                }
            }
        }
        .searchable(text: $search, prompt: "Search settings")
        .overlay {
            if matches.isEmpty {
                ContentUnavailableView(
                    "No settings match “\(search.trimmingCharacters(in: .whitespaces))”",
                    systemImage: "magnifyingglass",
                    description: Text("Try sound, folders or the name of a service.")
                )
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .background(Color(.systemGroupedBackground))
            }
        }
        .confirmationDialog("Erase all data?", isPresented: $confirmingErase, titleVisibility: .visible) {
            Button("Erase everything", role: .destructive) { library.eraseEverything() }
        } message: {
            Text("Every song Resonate lists is deleted from this iPhone, including those in folders you added; other files in those folders stay. This cannot be undone. Resonate closes once it is done; open it again to start fresh.")
        }
        .navigationTitle("Settings")
    }

    private var hidden: Set<SettingsPlace> {
        var hidden: Set<SettingsPlace> = []
        if !library.features.serving { hidden.insert(.server) }
        if !library.boundRoutes.contains(where: { $0.id != library.route?.id }) { hidden.insert(.otherOutputs) }
        return hidden
    }

    static func sleepSummary(_ timer: SleepTimer?, remaining: UInt64?) -> String {
        switch timer {
        case .after: remaining.map { "Pauses in \(clock($0 + 999))" } ?? "Pauses after the set time"
        case .endOfTrack: "Pauses at the end of this song"
        case .endOfQueue: "Pauses at the end of the queue"
        case nil: "No timer set"
        }
    }
}

private struct SettingsEntry: View {
    let match: SettingsMatch
    let searching: Bool
    let erase: () -> Void

    var body: some View {
        let place = match.place
        let said = searching ? match.said : nil
        switch place.opening {
        case .page(let destination):
            NavigationLink(value: destination) {
                SettingsStatusRow(match: match, searching: searching)
            }
        case .sleepTimer:
            SleepTimerButton(said: said)
        case .online:
            OnlineSwitch(said: said)
        case .version:
            ToolRow(title: place.title, detail: Self.version, symbol: place.symbol, detailLines: 1)
        case .erase:
            EraseRow(erase: erase)
        }
    }

    private static let version: String = {
        let info = Bundle.main.infoDictionary
        let shown = info?["CFBundleShortVersionString"] as? String ?? "Unknown"
        let build = info?["CFBundleVersion"] as? String
        return build.map { $0 == shown ? "Version \(shown)" : "Version \(shown) (\($0))" } ?? "Version \(shown)"
    }()
}

private struct SettingsStatusRow: View {
    @Environment(Library.self) private var library
    let match: SettingsMatch
    let searching: Bool

    var body: some View {
        let place = match.place
        let said = searching ? match.said : nil
        switch place {
        case .searchHistory:
            SearchHistoryStatus(said: said)
        case .notices:
            NoticesStatus(said: said)
        default:
            ToolRow(
                title: place.title,
                detail: searching ? match.detail(status: status) : status,
                symbol: place.symbol,
                detailLines: 1
            )
        }
    }

    private var status: String {
        let features = library.features
        let place = match.place
        switch place {
        case .equaliser:
            guard let preferences = library.preferences else { return place.summary }
            return preferences.equaliserEnabled ? "On · \(preferences.equaliser.summary)" : "Off"
        case .processing:
            switch library.preferences?.replayGain {
            case .track: return "Track ReplayGain"
            case .album: return "Album ReplayGain"
            default: return place.summary
            }
        case .output:
            return library.routeReading.outputs.first.map { "Playing through \($0.name)" } ?? place.summary
        case .folders:
            let folders = library.folderList
            let readable = folders.filter(\.readable).count
            return folders.isEmpty ? "Add and manage your music folders" : "\(counted(folders.count, "folder")) · \(readable) available"
        case .enrichment:
            guard features.online else { return "Needs online features" }
            let enrichment = features.enrichment
            guard enrichment.automatic else { return "Looked up when you ask" }
            return enrichment.unmeteredOnly ? "Looked up automatically, on Wi-Fi" : "Looked up automatically"
        case .providers:
            return providersDetail(features.downloadAccounts)
        case .scrobbling:
            let account = features.scrobbleAccount
            let services = [
                account?.listenbrainzToken.isEmpty == false ? "ListenBrainz" : nil,
                account?.lastfmSession.isEmpty == false ? "Last.fm" : nil,
            ].compactMap { $0 }
            return services.isEmpty ? "Share your listening with ListenBrainz or Last.fm" : services.joined(separator: " · ")
        case .sync:
            switch features.syncActivity {
            case .fetching(let songs, _): return "Fetching \(counted(songs, "song")) from your devices"
            case .syncing: return "Syncing…"
            case nil: break
            }
            let standing = syncStanding(features.syncStatus, config: features.syncConfig, online: features.online)
            return features.syncUpToDate ? "Up to date · " + standing : standing
        case .accent:
            return AppearanceState.shared.accent.map { $0.name.capitalized } ?? "Resonate"
        case .background:
            let appearance = AppearanceState.shared
            guard appearance.themedBackground else { return "Off" }
            return appearance.movingBackground ? "Flowing" : "Still"
        case .roomCorrection:
            return RoomCorrectionRow.status(of: features.room)
        default:
            return place.summary
        }
    }
}

private struct SearchHistoryStatus: View {
    @AppStorage(SearchModel.historyKeptKey, store: SearchHistoryStore.defaults) private var kept = true
    let said: String?

    var body: some View {
        ToolRow(
            title: SettingsPlace.searchHistory.title,
            detail: said ?? (kept ? "Recent searches are kept on this iPhone" : "Off"),
            symbol: SettingsPlace.searchHistory.symbol,
            detailLines: 1
        )
    }
}

private struct NoticesStatus: View {
    @AppStorage(Library.unaskedNoticesKey, store: Library.unaskedNoticeSettings) private var told = false
    let said: String?

    var body: some View {
        ToolRow(
            title: SettingsPlace.notices.title,
            detail: said ?? (told ? "Also what the library finds by itself" : "What you start, and every failure"),
            symbol: SettingsPlace.notices.symbol,
            detailLines: 1
        )
    }
}

private struct OnlineSwitch: View {
    @Environment(Library.self) private var library
    let said: String?

    var body: some View {
        let features = library.features
        Toggle(isOn: Binding(get: { features.online }, set: { features.setOnline($0) })) {
            ToolRow(
                title: SettingsPlace.online.title,
                detail: said ?? (features.online ? "Artwork, lyrics and music search are on" : "Resonate stays offline"),
                symbol: SettingsPlace.online.symbol,
                detailLines: 1
            )
        }
    }
}

private struct EraseRow: View {
    @Environment(Library.self) private var library
    let erase: () -> Void

    var body: some View {
        Button(role: .destructive, action: erase) {
            ToolRow(
                title: SettingsPlace.erase.title,
                detail: SettingsPlace.erase.summary,
                symbol: SettingsPlace.erase.symbol,
                ink: Color.failure,
                detailLines: 1
            )
        }
        .disabled(library.erasing)
    }
}

private struct SleepTimerButton: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    let said: String?

    var body: some View {
        Button { navigation.sleepTimer = true } label: { SleepTimerRow(said: said) }
            .disabled(library.playingTrack == nil)
    }
}

private struct SleepTimerRow: View {
    @Environment(Library.self) private var library
    let said: String?

    var body: some View {
        ToolRow(
            title: SettingsPlace.sleepTimer.title,
            detail: said ?? SettingsView.sleepSummary(library.sleepTimer, remaining: library.clock.sleepRemainingMs),
            symbol: SettingsPlace.sleepTimer.symbol,
            detailLines: 1
        )
    }
}

extension MusicProvider {
    var title: String {
        switch self {
        case .monochrome: "Monochrome"
        case .hifiApi: "hifi-api"
        case .subsonic: "Subsonic"
        case .jellyfin: "Jellyfin"
        case .tidal: "TIDAL"
        }
    }
}

struct CommittingSlider<Value: BinaryFloatingPoint & Sendable>: View where Value.Stride: BinaryFloatingPoint {
    let title: String
    let value: Value
    let range: ClosedRange<Value>
    let step: Value.Stride
    @Binding var editing: Value?
    let spoken: (Value) -> String
    let commit: (Value) -> Void
    @State private var dragging = false
    @State private var settling: Task<Void, Never>?

    private static var settle: Duration { .milliseconds(400) }

    var body: some View {
        Slider(
            value: Binding(get: { editing ?? value }, set: moved),
            in: range,
            step: step
        ) { active in
            dragging = active
            settling?.cancel()
            if !active, let editing { finish(editing) }
        }
        .accessibilityLabel(title)
        .accessibilityValue(spoken(editing ?? value))
    }

    private func moved(_ chosen: Value) {
        editing = chosen
        settling?.cancel()
        guard !dragging else { return }
        settling = Task {
            try? await Task.sleep(for: Self.settle)
            guard !Task.isCancelled, !dragging else { return }
            finish(chosen)
        }
    }

    private func finish(_ chosen: Value) {
        commit(chosen)
        editing = nil
    }
}

struct ProcessingView: View {
    @Environment(Library.self) private var library

    var body: some View {
        ThemedForm {
            if let settings = library.preferences {
                Section("Player volume") {
                    Slider(
                        value: Binding(
                            get: { Double(library.volume) },
                            set: { library.setVolume(Float($0)) }
                        ),
                        in: 0...1
                    )
                    .accessibilityLabel("Player volume")
                }
                Section {
                    Picker("ReplayGain", selection: binding(settings.replayGain) { $0.replayGain = $1 }) {
                        Text("Off").tag(GainMode.off)
                        Text("Track").tag(GainMode.track)
                        Text("Album").tag(GainMode.album)
                    }
                    Toggle("True-peak clip prevention", isOn: binding(settings.truePeak) { $0.truePeak = $1 })
                } footer: {
                    Text("Uses the gain tags in your files to balance loudness.")
                }
                Section("Resampling") {
                    Picker("Resampling quality", selection: binding(settings.quality) { $0.quality = $1 }) {
                        Text("Low latency").tag(ResamplingQuality.lowLatency)
                        Text("Balanced").tag(ResamplingQuality.balanced)
                        Text("High").tag(ResamplingQuality.high)
                        Text("Very high").tag(ResamplingQuality.veryHigh)
                    }
                    Picker("Filter phase", selection: binding(settings.phase) { $0.phase = $1 }) {
                        Text("Linear").tag(ResamplingPhase.linear)
                        Text("Intermediate").tag(ResamplingPhase.intermediate)
                        Text("Minimum").tag(ResamplingPhase.minimum)
                    }
                }
                Section {
                    Picker("Crossfeed", selection: binding(settings.crossfeed) { $0.crossfeed = $1 }) {
                        Text("Off").tag(CrossfeedLevel.off)
                        Text("Light").tag(CrossfeedLevel.light)
                        Text("Moderate").tag(CrossfeedLevel.moderate)
                        Text("Strong").tag(CrossfeedLevel.strong)
                    }
                } header: {
                    Text("Stereo image")
                } footer: {
                    Text(Self.crossfeedMeaning(settings.crossfeed) + " Only a stereo stream is crossfed; anything else plays as it is whatever this says. A crossfed stream is converted rather than bit-perfect.")
                }
                Section {
                    Picker("Lossy restoration", selection: binding(settings.restoration) { $0.restoration = $1 }) {
                        Text("Off").tag(LossyRestoration.off)
                        Text("Repair").tag(LossyRestoration.repair)
                        Text("Extend").tag(LossyRestoration.extend)
                    }
                } footer: {
                    Text("Experimental processing for MP3, AAC and Vorbis. Lossless music passes through unchanged by restoration.")
                }
            }
        }
        .navigationTitle("Sound processing")
    }

    private static func crossfeedMeaning(_ level: CrossfeedLevel) -> String {
        switch level {
        case .off: "Each channel stays in its own ear, exactly as the record was mixed."
        case .light: "A faint, low copy of each side reaches the other ear (650 Hz, 9.5 dB down), enough to take the edge off a hard-panned record."
        case .moderate: "The classic setting (700 Hz, 6 dB down): the image closes up toward the front without losing its width."
        case .strong: "The most of the other side reaches each ear (700 Hz, 4.5 dB down), for old stereo records with the voices on one side and the band on the other."
        }
    }

    private func binding<Value: Sendable>(
        _ value: Value,
        _ apply: @escaping @Sendable (inout PlaybackPreferences, Value) -> Void
    ) -> Binding<Value> {
        Binding(get: { value }, set: { chosen in
            library.updatePreferences { current in
                var next = current
                apply(&next, chosen)
                return next
            }
        })
    }
}

struct OutputView: View {
    @Environment(Library.self) private var library
    @State private var buffer: Double?
    @AppStorage(NowPlaying.followsSongRateKey, store: NowPlaying.playbackSettings) private var followsSongRate = true

    var body: some View {
        let reading = library.routeReading
        ThemedForm {
            Section("Route") {
                ForEach(reading.outputs, id: \.id) { output in
                    VStack(alignment: .leading) {
                        Text(output.name)
                        Text("Active route · \(output.channels) channels").font(.caption).foregroundStyle(.tint)
                    }
                }
                HStack {
                    Text("Choose a device")
                    Spacer()
                    RoutePicker().frame(width: 44, height: 44)
                }
            }
            Section {
                LabeledContent("Stream", value: "Stereo float · \(library.clock.outputRate > 0 ? library.clock.outputRate : UInt32(reading.sampleRate)) Hz")
                LabeledContent("Hardware rate", value: "\(Int(reading.sampleRate)) Hz")
                LabeledContent("Output latency", value: String(format: "%.1f ms", reading.latencyMs))
                LabeledContent("Underruns", value: "\(library.clock.underruns)")
                if library.clock.overs > 0 {
                    LabeledContent("Clamped samples", value: "\(library.clock.overs)")
                }
            } header: {
                Text("Current stream")
            } footer: {
                Text("iOS owns the physical route; the system mixer and the chosen device determine the output format.")
            }
            Section {
                Toggle("Match the song's sample rate", isOn: $followsSongRate)
                    .onChange(of: followsSongRate) { library.nowPlaying.songRateFollowingChanged() }
            } footer: {
                Text("Asks the route for each song's own rate, so a USB DAC that can follow it plays without resampling. The built-in speaker and most wireless routes keep their own rate. A change of rate between songs costs a short gap.")
            }
            if let settings = library.preferences {
                Section {
                    Text("Playback buffer: \(Int(buffer ?? Double(settings.bufferMs))) ms")
                    CommittingSlider(
                        title: "Playback buffer",
                        value: Double(settings.bufferMs),
                        range: 50...2000,
                        step: 50,
                        editing: $buffer,
                        spoken: { "\(Int($0)) milliseconds" }
                    ) { chosen in
                        library.updatePreferences { var next = $0; next.bufferMs = UInt32(chosen); return next }
                    }
                    Toggle("Match DSD loudness to PCM", isOn: Binding(
                        get: { settings.dsdLikePcm },
                        set: { value in library.updatePreferences { var next = $0; next.dsdLikePcm = value; return next } }
                    ))
                }
            }
        }
        .navigationTitle("Audio output")
    }
}

struct ScrobblingView: View {
    @Environment(Library.self) private var library
    @State private var account = ScrobbleAccount(listenbrainzToken: "", lastfmKey: "", lastfmSecret: "", lastfmSession: "")
    @State private var user = ""
    @State private var password = ""
    @State private var history = false

    var body: some View {
        let features = library.features
        ThemedForm {
            Section {
                SecureField("ListenBrainz token", text: $account.listenbrainzToken)
            } header: {
                Text("ListenBrainz")
            } footer: {
                Text("Completed listens are queued on this device and submitted when online features are enabled.")
            }
            Section("Last.fm") {
                SecureField("Last.fm application key", text: $account.lastfmKey)
                SecureField("Last.fm application secret", text: $account.lastfmSecret)
                SecureField("Last.fm session", text: $account.lastfmSession)
                TextField("Last.fm username", text: $user).textInputAutocapitalization(.never)
                SecureField("Last.fm password (used only to sign in)", text: $password)
                Button("Sign in to Last.fm") {
                    features.signInLastFm(account, user: user, password: password)
                    password = ""
                }
                .disabled(account.lastfmKey.isEmpty || account.lastfmSecret.isEmpty || user.isEmpty || password.isEmpty)
            }
            Section {
                Toggle("Include existing listening history", isOn: $history)
                Button("Save scrobbling accounts") { features.saveScrobbling(account, includeHistory: history) }
                Button("Validate accounts") { features.validateScrobbling(account) }
            } footer: {
                Text("Favourites with an identified recording are loved on each saved account, and taken back there when you remove them.")
            }
            if let status = features.scrobbleStatus {
                Text(status).foregroundStyle(.secondary)
            }
        }
        .navigationTitle("Scrobbling")
        .onAppear { if let saved = features.scrobbleAccount { account = saved } }
        .onChange(of: features.scrobbleAccount) { _, saved in if let saved { account = saved } }
    }
}

struct NoticesView: View {
    @AppStorage(Library.unaskedNoticesKey, store: Library.unaskedNoticeSettings) private var told = false

    var body: some View {
        ThemedForm {
            Section {
                Toggle(isOn: $told) {
                    ToolRow(
                        title: "Tell me what the library finds by itself",
                        detail: "New songs a background scan found, songs hidden as gone and songs kept from your devices",
                        symbol: "bell.badge"
                    )
                }
            } footer: {
                Text("A scan you start and every failure are always told.")
            }
        }
        .navigationTitle("Notices")
    }
}

@MainActor
enum SearchHistoryStore {
    static let defaults = UserDefaults(suiteName: SearchModel.historySuite)
}

struct SearchHistoryView: View {
    @Environment(Library.self) private var library
    @AppStorage(SearchModel.historyKeptKey, store: SearchHistoryStore.defaults) private var kept = true

    var body: some View {
        ThemedForm {
            Section {
                Toggle(isOn: $kept) {
                    ToolRow(title: "Keep search history", detail: "Offer your recent searches again", symbol: "clock.arrow.circlepath")
                }
                .onChange(of: kept) { _, now in
                    guard !now else { return }
                    library.songSearch.forgetRecents()
                    library.tabSearch.forgetRecents()
                }
            } footer: {
                Text("Searches are kept on this iPhone only. Turning this off forgets the ones kept.")
            }
        }
        .navigationTitle("Search history")
    }
}
