import AVFoundation
import ResonateNative
import SwiftUI

struct SettingsView: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    @State private var search = ""

    var body: some View {
        let features = library.features
        let preferences = library.preferences
        let processing: String =
            switch preferences?.replayGain {
            case .track: "Track ReplayGain"
            case .album: "Album ReplayGain"
            default: "ReplayGain, resampling and clip prevention"
            }
        List {
            Section("Sound and playback") {
                if shown("Equaliser", "curve", "preamp") {
                    NavigationLink(value: Destination.sound) {
                        ToolRow(
                            title: "Equaliser",
                            detail: preferences?.equaliserEnabled == true
                                ? (preferences?.profileName.isEmpty == false ? preferences?.profileName ?? "" : "Custom ten-band curve")
                                : "Ten-band curve and preamp",
                            symbol: "slider.vertical.3"
                        )
                    }
                }
                if shown("Sound processing", "ReplayGain", "resampling") {
                    NavigationLink(value: Destination.processing) {
                        ToolRow(
                            title: "Sound processing",
                            detail: processing,
                            symbol: "dial.medium"
                        )
                    }
                }
                if shown("Audio output", "buffer", "route") {
                    NavigationLink(value: Destination.output) {
                        ToolRow(title: "Audio output", detail: "Choose a device and adjust buffering", symbol: "hifispeaker")
                    }
                }
                if shown("Headphone correction", "AutoEq") {
                    NavigationLink(value: Destination.autoEq) {
                        ToolRow(title: "Headphone correction", detail: "Find and apply an AutoEq profile", symbol: "headphones")
                    }
                }
                if shown("Sleep timer") {
                    Button { navigation.sleepTimer = true } label: {
                        ToolRow(title: "Sleep timer", detail: library.snapshot?.sleep == nil ? "Pause playback later" : "A timer is running", symbol: "moon")
                    }
                    .disabled(library.snapshot?.current == nil)
                }
            }
            Section("Library sources") {
                if shown("Music folders") {
                    NavigationLink(value: Destination.folders) {
                        ToolRow(title: "Music folders", detail: "Add and manage your music folders", symbol: "folder")
                    }
                }
                if shown("Rescan library") {
                    Button { library.rescan() } label: {
                        ToolRow(title: "Rescan library", detail: library.scanning ? "Reading your music…" : "Find new and changed music in your folders", symbol: "arrow.clockwise")
                    }
                    .disabled(library.scanning)
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
                if shown("Device sync", "devices") {
                    NavigationLink(value: Destination.sync) {
                        ToolRow(title: "Device sync", detail: features.syncStatus.map(\.standing.title) ?? "Keep your devices in step", symbol: "arrow.triangle.2.circlepath")
                    }
                }
            }
        }
        .buttonStyle(.plain)
        .searchable(text: $search, prompt: "Search settings")
        .navigationTitle("Settings")
    }

    private func shown(_ words: String...) -> Bool {
        let folded = search.lowercased()
        return folded.isEmpty || words.contains { $0.lowercased().contains(folded) }
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

extension SyncStanding {
    var title: String {
        switch self {
        case .off: "Off"
        case .connecting: "Connecting…"
        case .connected: "Connected"
        case .unreachable: "Server unreachable"
        case .refused: "Refused by the server"
        }
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
            Slider(
                value: Binding(get: { editing ?? value }, set: { editing = $0 }),
                in: range,
                step: 0.5
            ) { active in
                if !active, let editing {
                    commit(editing)
                    self.editing = nil
                }
            }
            .accessibilityLabel(title)
        }
    }
}

struct SoundSettingsView: View {
    @Environment(Library.self) private var library
    @State private var importing = false
    @State private var profileName = ""
    @State private var profileText = ""
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
                    Button("Reset curve") {
                        library.updatePreferences { current in
                            var next = current
                            next.preampDb = 0
                            next.gainsDb = current.gainsDb.map { _ in 0 }
                            next.profileText = ""
                            next.profileName = ""
                            return next
                        }
                    }
                }
                Section {
                    Button(importing ? "Close profile import" : "Import an equaliser profile") { importing.toggle() }
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
                    Text("Experimental: repairs or extends what a lossy encoder removed.")
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
                LabeledContent("Stream", value: "Stereo float · \(library.snapshot?.outputRate ?? UInt32(session.sampleRate)) Hz")
                LabeledContent("Hardware rate", value: "\(Int(session.sampleRate)) Hz")
                LabeledContent("Output latency", value: String(format: "%.1f ms", (session.outputLatency + session.ioBufferDuration) * 1000))
                LabeledContent("Underruns", value: "\(library.snapshot?.underruns ?? 0)")
            } header: {
                Text("Current stream")
            } footer: {
                Text("iOS owns the physical route; the system mixer and the chosen device determine the output format.")
            }
            if let settings = library.preferences {
                Section {
                    let shown = buffer ?? Double(settings.bufferMs)
                    Text("Playback buffer: \(Int(shown)) ms")
                    Slider(
                        value: Binding(get: { shown }, set: { buffer = $0 }),
                        in: 50...2000,
                        step: 50
                    ) { active in
                        if !active, let chosen = buffer {
                            library.updatePreferences { var next = $0; next.bufferMs = UInt32(chosen); return next }
                            buffer = nil
                        }
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
            if features.correctionBusy { ProgressView() }
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
            }
        }
        .navigationTitle("AutoEq headphone correction")
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
