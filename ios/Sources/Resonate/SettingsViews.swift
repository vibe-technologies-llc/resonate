import AVFoundation
import ResonateNative
import SwiftUI

enum SettingsOpening {
    case page(Destination)
    case sleepTimer
    case rescan
}

struct SettingsEntry: Identifiable {
    let title: String
    let detail: String
    let symbol: String
    let keywords: String
    let opening: SettingsOpening
    var available = true

    var id: String { title }
    var searched: String { "\(title) \(detail) \(keywords)" }
}

struct SettingsView: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    @State private var search = ""
    private static let unlistedWords = [
        "Online features internet network artwork covers lyrics enrichment music search",
        "Download providers TIDAL Subsonic Monochrome",
        "Scrobbling ListenBrainz Last.fm",
    ]

    var body: some View {
        let features = library.features
        let sound = soundEntries.filter { matches($0.searched) }
        let sources = sourceEntries.filter { matches($0.searched) }
        List {
            if !sound.isEmpty {
                Section("Sound and playback") {
                    ForEach(sound) { entryRow($0) }
                }
            }
            if !sources.isEmpty {
                Section("Library sources") {
                    ForEach(sources) { entryRow($0) }
                }
            }
            Section("Connected services") {
                if shown("Online features", "internet") {
                    Toggle(isOn: Binding(get: { features.online }, set: { features.setOnline($0) })) {
                        ToolRow(title: "Online features", detail: "MusicBrainz, artwork, lyrics, AutoEq and recognition", symbol: "globe")
                    }
                }
                if shown("Download providers", "TIDAL", "Subsonic", "Monochrome") {
                    NavigationLink(value: Destination.providers) {
                        ToolRow(title: "Download providers", detail: features.providerAccount?.provider.title ?? "Monochrome", symbol: "arrow.down.circle")
                    }
                }
                if shown("Scrobbling", "ListenBrainz", "Last.fm") {
                    NavigationLink(value: Destination.scrobbling) {
                        ToolRow(title: "Scrobbling", detail: scrobblingDetail(features.scrobbleAccount), symbol: "dot.radiowaves.left.and.right")
                    }
                }
                if matches(syncEntry.searched) {
                    entryRow(syncEntry)
                }
            }
        }
        .buttonStyle(.plain)
        .searchable(text: $search, prompt: "Search settings")
        .overlay {
            if nothingMatches {
                ContentUnavailableView(
                    "No settings match “\(search.trimmingCharacters(in: .whitespaces))”",
                    systemImage: "magnifyingglass",
                    description: Text("Try another word, such as equaliser, folders or sync.")
                )
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .background(Color(.systemGroupedBackground))
            }
        }
        .navigationTitle("Settings")
    }

    private var soundEntries: [SettingsEntry] {
        let preferences = library.preferences
        return [
            SettingsEntry(
                title: "Equaliser",
                detail: Self.equaliserSummary(preferences),
                symbol: "slider.vertical.3",
                keywords: "sound playback curve preamp bands profile",
                opening: .page(.sound)
            ),
            SettingsEntry(
                title: "Sound processing",
                detail: Self.processingSummary(preferences),
                symbol: "dial.medium",
                keywords: "sound playback ReplayGain resampling clip true peak restoration volume",
                opening: .page(.processing)
            ),
            SettingsEntry(
                title: "Audio output",
                detail: "Choose a device and adjust buffering",
                symbol: "hifispeaker",
                keywords: "sound playback route bluetooth AirPlay buffer DSD",
                opening: .page(.output)
            ),
            SettingsEntry(
                title: "Headphone correction",
                detail: "Find and apply an AutoEq profile",
                symbol: "headphones",
                keywords: "sound playback AutoEq equaliser correction",
                opening: .page(.autoEq)
            ),
            SettingsEntry(
                title: "Sleep timer",
                detail: Self.sleepSummary(library.snapshot?.sleep, remaining: library.clock.sleepRemainingMs),
                symbol: "moon",
                keywords: "sound playback pause minutes end song queue",
                opening: .sleepTimer,
                available: library.snapshot?.current != nil
            ),
        ]
    }

    private var sourceEntries: [SettingsEntry] {
        let folders = library.folderList
        let readable = folders.filter(\.readable).count
        let rescanDetail =
            library.scanning ? "Reading your music…"
            : readable == 0 ? "Add a music folder first"
            : "Find new and changed music in your folders"
        return [
            SettingsEntry(
                title: "Music folders",
                detail: folders.isEmpty ? "Add and manage your music folders" : "\(counted(folders.count, "folder")) · \(readable) available",
                symbol: "folder",
                keywords: "library sources storage add remove rescan Files",
                opening: .page(.folders)
            ),
            SettingsEntry(
                title: "Rescan library",
                detail: rescanDetail,
                symbol: "arrow.clockwise",
                keywords: "library sources refresh scan files",
                opening: .rescan,
                available: !library.scanning && readable > 0
            ),
        ]
    }

    private var syncEntry: SettingsEntry {
        let features = library.features
        return SettingsEntry(
            title: "Device sync",
            detail: syncStanding(features.syncStatus, config: features.syncConfig, online: features.online),
            symbol: "arrow.triangle.2.circlepath",
            keywords: "connected services devices pairing code phone computer playlists favourites listening history handoff",
            opening: .page(.sync)
        )
    }

    @ViewBuilder
    private func entryRow(_ entry: SettingsEntry) -> some View {
        let row = ToolRow(title: entry.title, detail: entry.detail, symbol: entry.symbol)
        switch entry.opening {
        case .page(let destination):
            NavigationLink(value: destination) { row }
                .disabled(!entry.available)
        case .sleepTimer:
            Button { navigation.sleepTimer = true } label: { row }
                .disabled(!entry.available)
        case .rescan:
            Button { library.rescan() } label: { row }
                .disabled(!entry.available)
        }
    }

    private var terms: [String] {
        search.split(whereSeparator: \.isWhitespace).map(String.init)
    }

    private var nothingMatches: Bool {
        let listed = (soundEntries + sourceEntries + [syncEntry]).map(\.searched)
        return !terms.isEmpty && !(listed + Self.unlistedWords).contains(where: matches)
    }

    private func matches(_ searched: String) -> Bool {
        terms.allSatisfy { searched.localizedCaseInsensitiveContains($0) }
    }

    private func shown(_ words: String...) -> Bool {
        matches(words.joined(separator: " "))
    }

    private static func equaliserSummary(_ preferences: PlaybackPreferences?) -> String {
        guard let preferences, preferences.equaliserEnabled else { return "Ten-band curve and preamp" }
        return preferences.profileName.isEmpty ? "Custom ten-band curve" : preferences.profileName
    }

    private static func processingSummary(_ preferences: PlaybackPreferences?) -> String {
        switch preferences?.replayGain {
        case .track: "Track ReplayGain"
        case .album: "Album ReplayGain"
        default: "ReplayGain, resampling and clip prevention"
        }
    }

    private static func sleepSummary(_ timer: SleepTimer?, remaining: UInt64?) -> String {
        switch timer {
        case .after: "Pauses in \(clock((remaining ?? 0) + 999))"
        case .endOfTrack: "Pauses at the end of this song"
        case .endOfQueue: "Pauses at the end of the queue"
        case nil: "No timer set"
        }
    }

    private func scrobblingDetail(_ account: ScrobbleAccount?) -> String {
        let services = [
            account?.listenbrainzToken.isEmpty == false ? "ListenBrainz" : nil,
            account?.lastfmSession.isEmpty == false ? "Last.fm" : nil,
        ].compactMap { $0 }
        return services.isEmpty ? "Share your listening with ListenBrainz or Last.fm" : services.joined(separator: " · ")
    }
}

extension MusicProvider {
    var title: String {
        switch self {
        case .monochrome: "Monochrome"
        case .hifiApi: "hifi-api"
        case .subsonic: "Subsonic"
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

struct LevelSlider: View {
    let title: String
    let value: Float
    let range: ClosedRange<Float>
    let commit: (Float) -> Void
    @State private var editing: Float?

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            HStack {
                Text(title)
                Spacer()
                Text(String(format: "%+.1f dB", editing ?? value)).monospacedDigit().foregroundStyle(.secondary)
            }
            CommittingSlider(title: title, value: value, range: range, step: 0.5, editing: $editing, commit: commit)
        }
    }
}

struct SoundSettingsView: View {
    @Environment(Library.self) private var library
    @State private var importing = false
    @State private var profileName = ""
    @State private var profileText = ""
    @State private var confirmingReset = false
    private static let frequencies = equaliserFrequencies()

    var body: some View {
        Form {
            if let settings = library.preferences {
                Section {
                    Toggle("Enable equaliser", isOn: Binding(
                        get: { settings.equaliserEnabled },
                        set: { enabled in library.updatePreferences { var next = $0; next.equaliserEnabled = enabled; return next } }
                    ))
                    if !settings.profileText.isEmpty {
                        Text("Active profile: \(settings.profileName)").foregroundStyle(.tint)
                    }
                } footer: {
                    Text("This curve applies to every iOS audio route.")
                }
                Section {
                    LevelSlider(title: "Preamp", value: settings.preampDb, range: -24...12) { gain in
                        library.updatePreferences { var next = $0; next.preampDb = gain; next.profileText = ""; next.profileName = ""; return next }
                    }
                    ForEach(Array(Self.frequencies.enumerated()), id: \.offset) { band, frequency in
                        LevelSlider(title: Self.label(frequency), value: settings.gainsDb[band], range: -12...12) { gain in
                            library.updatePreferences { current in
                                var next = current
                                next.gainsDb[band] = gain
                                next.profileText = ""
                                next.profileName = ""
                                return next
                            }
                        }
                    }
                    Button("Reset curve", role: .destructive) { confirmingReset = true }
                }
                Section {
                    Button(importing ? "Close profile import" : "Import an equaliser profile") {
                        if !importing, profileText.isEmpty {
                            profileName = settings.profileName
                            profileText = settings.profileText
                        }
                        importing.toggle()
                    }
                    if importing {
                        TextField("Profile name", text: $profileName)
                        TextField("EqualizerAPO / GraphicEQ profile", text: $profileText, axis: .vertical)
                            .lineLimit(3...12)
                            .font(.footnote.monospaced())
                        Button("Apply profile") {
                            library.features.applyProfileText(name: profileName.isEmpty ? "Imported profile" : profileName, text: profileText)
                        }
                        .disabled(profileText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                    }
                }
            }
        }
        .navigationTitle("Equaliser")
        .confirmationDialog("Reset the curve?", isPresented: $confirmingReset, titleVisibility: .visible) {
            Button("Reset curve", role: .destructive, action: resetCurve)
        } message: {
            Text("The preamp and every band return to 0 dB, and the active profile is cleared.")
        }
    }

    private func resetCurve() {
        library.updatePreferences { current in
            var next = current
            next.preampDb = 0
            next.gainsDb = current.gainsDb.map { _ in 0 }
            next.profileText = ""
            next.profileName = ""
            return next
        }
    }

    private static func label(_ frequency: Double) -> String {
        frequency < 1000 ? String(format: "%g Hz", frequency) : String(format: "%g kHz", frequency / 1000)
    }
}

struct ProcessingView: View {
    @Environment(Library.self) private var library

    var body: some View {
        Form {
            if let settings = library.preferences {
                Section("Player volume") {
                    Slider(
                        value: Binding(
                            get: { Double(library.snapshot?.volume ?? 1) },
                            set: { value in library.request { try $0.setVolume(volume: Float(value)) } }
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
                        Text("Fast").tag(ResamplingQuality.fast)
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

    var body: some View {
        let session = AVAudioSession.sharedInstance()
        let route = session.currentRoute.outputs
        Form {
            Section("Route") {
                ForEach(route, id: \.uid) { output in
                    VStack(alignment: .leading) {
                        Text(output.portName)
                        Text("Active route · \(output.channels?.count ?? 2) channels").font(.caption).foregroundStyle(.tint)
                    }
                }
                HStack {
                    Text("Choose a device")
                    Spacer()
                    RoutePicker().frame(width: 44, height: 44)
                }
            }
            Section {
                LabeledContent("Stream", value: "Stereo float · \(library.clock.outputRate > 0 ? library.clock.outputRate : UInt32(session.sampleRate)) Hz")
                LabeledContent("Hardware rate", value: "\(Int(session.sampleRate)) Hz")
                LabeledContent("Output latency", value: String(format: "%.1f ms", (session.outputLatency + session.ioBufferDuration) * 1000))
                LabeledContent("Underruns", value: "\(library.clock.underruns)")
            } header: {
                Text("Current stream")
            } footer: {
                Text("iOS owns the physical route; the system mixer and the chosen device determine the output format.")
            }
            if let settings = library.preferences {
                Section {
                    Text("Playback buffer: \(Int(buffer ?? Double(settings.bufferMs))) ms")
                    CommittingSlider(
                        title: "Playback buffer",
                        value: Double(settings.bufferMs),
                        range: 50...2000,
                        step: 50,
                        editing: $buffer
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

struct AutoEqView: View {
    @Environment(Library.self) private var library
    @State private var model = ""

    var body: some View {
        let features = library.features
        Form {
            Section {
                TextField("Headphone model", text: $model)
                    .onSubmit { features.searchCorrections(model) }
                Button("Find correction") { features.searchCorrections(model) }
                    .disabled(!features.online || model.isEmpty || features.correctionBusy)
                if !features.online {
                    Text("Enable online features in Settings to fetch corrections.").foregroundStyle(.secondary)
                }
            } footer: {
                Text("A selected profile applies to every route. Switch it off or edit the ten-band curve in Equaliser settings.")
            }
            if features.searchingCorrections {
                HStack(spacing: 12) {
                    ProgressView()
                    Text("Searching the AutoEq catalogue…").foregroundStyle(.secondary)
                }
            } else if let searched = features.correctionsFor, features.corrections.isEmpty {
                ContentUnavailableView(
                    "No headphones match “\(searched)”",
                    systemImage: "headphones",
                    description: Text("Try the model name as printed on the headphones.")
                )
            }
            Section {
                ForEach(features.corrections, id: \.id) { device in
                    Button {
                        features.applyCorrection(device)
                    } label: {
                        HStack {
                            Text(device.name)
                            Spacer()
                            Text("Apply correction").foregroundStyle(.tint)
                        }
                    }
                    .disabled(features.correctionBusy)
                }
            }
        }
        .navigationTitle("AutoEq")
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
        Form {
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
            }
            Section {
                Button("Send current favourites to saved accounts") { features.sendFavourites() }
            } footer: {
                Text("Sends favourites with identified recordings. Removing a favourite locally does not remove it from your online accounts.")
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
