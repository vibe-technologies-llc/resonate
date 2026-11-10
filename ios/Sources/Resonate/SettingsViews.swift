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
                            Text("settings.erase.footer")
                        }
                    }
                }
            }
        }
        .searchable(text: $search, prompt: "settings.search")
        .overlay {
            if matches.isEmpty {
                ContentUnavailableView(
                    said("settings.search.none", search.trimmingCharacters(in: .whitespaces)),
                    systemImage: "magnifyingglass",
                    description: Text("settings.search.none.detail")
                )
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .background(Color(.systemGroupedBackground))
            }
        }
        .confirmationDialog("settings.erase.confirm", isPresented: $confirmingErase, titleVisibility: .visible) {
            Button("settings.erase.confirm.action", role: .destructive) { library.eraseEverything() }
        } message: {
            Text("settings.erase.confirm.message")
        }
        .navigationTitle("settings.title")
    }

    private var hidden: Set<SettingsPlace> {
        var hidden: Set<SettingsPlace> = []
        if !library.features.serving { hidden.insert(.server) }
        if !library.boundRoutes.contains(where: { $0.id != library.route?.id }) { hidden.insert(.otherOutputs) }
        return hidden
    }

    static func sleepSummary(_ timer: SleepTimer?, remaining: UInt64?) -> String {
        switch timer {
        case .after: remaining.map { said("settings.sleep.pauses-in", clock($0 + 999)) } ?? said("settings.sleep.after")
        case .endOfTrack: said("settings.sleep.end-of-song")
        case .endOfQueue: said("settings.sleep.end-of-queue")
        case nil: said("settings.sleep.none")
        }
    }
}

private struct SettingsEntry: View {
    let match: SettingsMatch
    let searching: Bool
    let erase: () -> Void

    var body: some View {
        let place = match.place
        let matched = searching ? match.said : nil
        switch place.opening {
        case .page(let destination):
            NavigationLink(value: destination) {
                SettingsStatusRow(match: match, searching: searching)
            }
        case .sleepTimer:
            SleepTimerButton(matched: matched)
        case .online:
            OnlineSwitch(matched: matched)
        case .version:
            ToolRow(title: place.title, detail: Self.version, symbol: place.symbol, detailLines: 1)
        case .erase:
            EraseRow(erase: erase)
        }
    }

    private static var version: String {
        let info = Bundle.main.infoDictionary
        let shown = info?["CFBundleShortVersionString"] as? String ?? said("settings.version.unknown")
        let build = info?["CFBundleVersion"] as? String
        return build.map { $0 == shown ? said("settings.version", shown) : said("settings.version.build", shown, $0) } ?? said("settings.version", shown)
    }
}

private struct SettingsStatusRow: View {
    @Environment(Library.self) private var library
    let match: SettingsMatch
    let searching: Bool

    var body: some View {
        let place = match.place
        let matched = searching ? match.said : nil
        switch place {
        case .searchHistory:
            SearchHistoryStatus(matched: matched)
        case .notices:
            NoticesStatus(matched: matched)
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
            return preferences.equaliserEnabled ? said("settings.equaliser.on", preferences.equaliser.summary) : said("common.off")
        case .processing:
            switch library.preferences?.replayGain {
            case .track: return said("settings.processing.track-gain")
            case .album: return said("settings.processing.album-gain")
            default: return place.summary
            }
        case .output:
            return library.routeReading.outputs.first.map { said("settings.output.playing-through", $0.name) } ?? place.summary
        case .folders:
            let folders = library.folderList
            let readable = folders.filter(\.readable).count
            return folders.isEmpty ? said("settings.folders.none") : said("settings.folders.available", folders.count, readable)
        case .enrichment:
            guard features.online else { return said("settings.enrichment.offline") }
            let enrichment = features.enrichment
            guard enrichment.automatic else { return said("settings.enrichment.asked") }
            return said(enrichment.unmeteredOnly ? "settings.enrichment.automatic-wifi" : "settings.enrichment.automatic")
        case .providers:
            return providersDetail(features.downloadAccounts)
        case .scrobbling:
            let account = features.scrobbleAccount
            let services = [
                account?.listenbrainzToken.isEmpty == false ? said("service.listenbrainz") : nil,
                account?.lastfmSession.isEmpty == false ? said("service.lastfm") : nil,
            ].compactMap { $0 }
            return services.isEmpty ? said("settings.scrobbling.none") : services.joined(separator: " · ")
        case .sync:
            switch features.syncActivity {
            case .fetching(let songs, _): return said("settings.sync.fetching", songs)
            case .syncing: return said("sync.syncing")
            case nil: break
            }
            let standing = syncStanding(features.syncStatus, config: features.syncConfig, online: features.online)
            return features.syncUpToDate ? said("settings.sync.up-to-date", standing) : standing
        case .accent:
            return AppearanceState.shared.accent.map { $0.name.capitalized } ?? "Resonate"
        case .background:
            let appearance = AppearanceState.shared
            guard appearance.themedBackground else { return said("common.off") }
            return said(appearance.movingBackground ? "settings.background.flowing" : "settings.background.still")
        case .roomCorrection:
            return RoomCorrectionRow.status(of: features.room)
        default:
            return place.summary
        }
    }
}

private struct SearchHistoryStatus: View {
    @AppStorage(SearchModel.historyKeptKey, store: SearchHistoryStore.defaults) private var kept = true
    let matched: String?

    var body: some View {
        ToolRow(
            title: SettingsPlace.searchHistory.title,
            detail: matched ?? said(kept ? "settings.search-history.kept" : "common.off"),
            symbol: SettingsPlace.searchHistory.symbol,
            detailLines: 1
        )
    }
}

private struct NoticesStatus: View {
    @AppStorage(Library.unaskedNoticesKey, store: Library.unaskedNoticeSettings) private var told = false
    let matched: String?

    var body: some View {
        ToolRow(
            title: SettingsPlace.notices.title,
            detail: matched ?? said(told ? "settings.notices.unasked" : "settings.notices.asked"),
            symbol: SettingsPlace.notices.symbol,
            detailLines: 1
        )
    }
}

private struct OnlineSwitch: View {
    @Environment(Library.self) private var library
    let matched: String?

    var body: some View {
        let features = library.features
        Toggle(isOn: Binding(get: { features.online }, set: { features.setOnline($0) })) {
            ToolRow(
                title: SettingsPlace.online.title,
                detail: matched ?? said(features.online ? "settings.online.on" : "settings.online.off"),
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
    let matched: String?

    var body: some View {
        Button { navigation.sleepTimer = true } label: { SleepTimerRow(matched: matched) }
            .disabled(library.playingTrack == nil)
    }
}

private struct SleepTimerRow: View {
    @Environment(Library.self) private var library
    let matched: String?

    var body: some View {
        ToolRow(
            title: SettingsPlace.sleepTimer.title,
            detail: matched ?? SettingsView.sleepSummary(library.sleepTimer, remaining: library.clock.sleepRemainingMs),
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
                Section("processing.volume") {
                    Slider(
                        value: Binding(
                            get: { Double(library.volume) },
                            set: { library.setVolume(Float($0)) }
                        ),
                        in: 0...1
                    )
                    .accessibilityLabel("processing.volume")
                }
                Section {
                    Picker("processing.replay-gain", selection: binding(settings.replayGain) { $0.replayGain = $1 }) {
                        Text("common.off").tag(GainMode.off)
                        Text("processing.replay-gain.track").tag(GainMode.track)
                        Text("processing.replay-gain.album").tag(GainMode.album)
                    }
                    Toggle("processing.true-peak", isOn: binding(settings.truePeak) { $0.truePeak = $1 })
                } footer: {
                    Text("processing.replay-gain.footer")
                }
                Section("processing.resampling") {
                    Picker("processing.resampling.quality", selection: binding(settings.quality) { $0.quality = $1 }) {
                        Text("processing.resampling.low-latency").tag(ResamplingQuality.lowLatency)
                        Text("processing.resampling.balanced").tag(ResamplingQuality.balanced)
                        Text("processing.resampling.high").tag(ResamplingQuality.high)
                        Text("processing.resampling.very-high").tag(ResamplingQuality.veryHigh)
                    }
                    Picker("processing.phase", selection: binding(settings.phase) { $0.phase = $1 }) {
                        Text("processing.phase.linear").tag(ResamplingPhase.linear)
                        Text("processing.phase.intermediate").tag(ResamplingPhase.intermediate)
                        Text("processing.phase.minimum").tag(ResamplingPhase.minimum)
                    }
                }
                Section {
                    Picker("processing.crossfeed", selection: binding(settings.crossfeed) { $0.crossfeed = $1 }) {
                        Text("common.off").tag(CrossfeedLevel.off)
                        Text("processing.crossfeed.light").tag(CrossfeedLevel.light)
                        Text("processing.crossfeed.moderate").tag(CrossfeedLevel.moderate)
                        Text("processing.crossfeed.strong").tag(CrossfeedLevel.strong)
                    }
                } header: {
                    Text("processing.crossfeed.header")
                } footer: {
                    Text(said("processing.crossfeed.footer", Self.crossfeedMeaning(settings.crossfeed)))
                }
                Section {
                    Picker("processing.restoration", selection: binding(settings.restoration) { $0.restoration = $1 }) {
                        Text("common.off").tag(LossyRestoration.off)
                        Text("processing.restoration.repair").tag(LossyRestoration.repair)
                        Text("processing.restoration.extend").tag(LossyRestoration.extend)
                    }
                } footer: {
                    Text("processing.restoration.footer")
                }
            }
        }
        .navigationTitle("processing.title")
    }

    private static func crossfeedMeaning(_ level: CrossfeedLevel) -> String {
        switch level {
        case .off: said("processing.crossfeed.off.meaning")
        case .light: said("processing.crossfeed.light.meaning")
        case .moderate: said("processing.crossfeed.moderate.meaning")
        case .strong: said("processing.crossfeed.strong.meaning")
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
            Section("output.route") {
                ForEach(reading.outputs, id: \.id) { output in
                    VStack(alignment: .leading) {
                        Text(output.name)
                        Text(said("output.route.active", output.channels)).font(.caption).foregroundStyle(.tint)
                    }
                }
                HStack {
                    Text("output.route.choose")
                    Spacer()
                    RoutePicker().frame(width: 44, height: 44)
                }
            }
            Section {
                LabeledContent("output.stream", value: said("output.stream.value", Int(library.clock.outputRate > 0 ? library.clock.outputRate : UInt32(reading.sampleRate))))
                LabeledContent("output.hardware-rate", value: said("output.hertz", Int(reading.sampleRate)))
                LabeledContent("output.latency", value: said("output.milliseconds", String(format: "%.1f", reading.latencyMs)))
                LabeledContent("output.underruns", value: "\(library.clock.underruns)")
                if library.clock.overs > 0 {
                    LabeledContent("output.clamped", value: "\(library.clock.overs)")
                }
            } header: {
                Text("output.current-stream")
            } footer: {
                Text("output.current-stream.footer")
            }
            Section {
                Toggle("output.follow-song-rate", isOn: $followsSongRate)
                    .onChange(of: followsSongRate) { library.nowPlaying.songRateFollowingChanged() }
            } footer: {
                Text("output.follow-song-rate.footer")
            }
            if let settings = library.preferences {
                Section {
                    Text(said("output.buffer.value", Int(buffer ?? Double(settings.bufferMs))))
                    CommittingSlider(
                        title: said("output.buffer"),
                        value: Double(settings.bufferMs),
                        range: 50...2000,
                        step: 50,
                        editing: $buffer,
                        spoken: { said("output.buffer.spoken", Int($0)) }
                    ) { chosen in
                        library.updatePreferences { var next = $0; next.bufferMs = UInt32(chosen); return next }
                    }
                    Toggle("output.dsd-like-pcm", isOn: Binding(
                        get: { settings.dsdLikePcm },
                        set: { value in library.updatePreferences { var next = $0; next.dsdLikePcm = value; return next } }
                    ))
                }
            }
        }
        .navigationTitle("output.title")
    }
}

struct ScrobblingView: View {
    @Environment(Library.self) private var library
    @Environment(\.openURL) private var openURL
    @State private var account = ScrobbleAccount(listenbrainzToken: "", lastfmKey: "", lastfmSecret: "", lastfmSession: "")
    @State private var history = false

    var body: some View {
        let features = library.features
        ThemedForm {
            Section {
                SecureField("scrobbling.listenbrainz.token", text: $account.listenbrainzToken)
            } header: {
                Text("service.listenbrainz")
            } footer: {
                Text("scrobbling.listenbrainz.footer")
            }
            Section("service.lastfm") {
                SecureField("scrobbling.lastfm.key", text: $account.lastfmKey)
                SecureField("scrobbling.lastfm.secret", text: $account.lastfmSecret)
                SecureField("scrobbling.lastfm.session", text: $account.lastfmSession)
                Button(said(features.lastfmBusy ? "scrobbling.lastfm.waiting" : "scrobbling.lastfm.sign-in")) {
                    features.signInLastFm(account)
                }
                .disabled(account.lastfmKey.isEmpty || account.lastfmSecret.isEmpty || features.lastfmBusy)
                if features.lastfmBusy {
                    Button("scrobbling.lastfm.cancel", role: .cancel) { features.cancelLastFmSignIn() }
                }
                if let address = features.lastfmAddress, let url = URL(string: address) {
                    Button("scrobbling.lastfm.open-again") { openURL(url) }
                    Text("scrobbling.lastfm.allow")
                        .foregroundStyle(.secondary)
                }
            }
            .onChange(of: features.lastfmAddress) { _, address in
                if let address, let url = URL(string: address) { openURL(url) }
            }
            Section {
                Toggle("scrobbling.include-history", isOn: $history)
                Button("scrobbling.save") { features.saveScrobbling(account, includeHistory: history) }
                Button("scrobbling.validate") { features.validateScrobbling(account) }
            } footer: {
                Text("scrobbling.footer")
            }
            if let status = features.scrobbleStatus {
                Text(status).foregroundStyle(.secondary)
            }
        }
        .navigationTitle("scrobbling.title")
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
                        title: said("notices.unasked"),
                        detail: said("notices.unasked.detail"),
                        symbol: "bell.badge"
                    )
                }
            } footer: {
                Text("notices.footer")
            }
        }
        .navigationTitle("notices.title")
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
                    ToolRow(title: said("search-history.keep"), detail: said("search-history.keep.detail"), symbol: "clock.arrow.circlepath")
                }
                .onChange(of: kept) { _, now in
                    guard !now else { return }
                    library.songSearch.forgetRecents()
                    library.tabSearch.forgetRecents()
                }
            } footer: {
                Text("search-history.footer")
            }
        }
        .navigationTitle("search-history.title")
    }
}
