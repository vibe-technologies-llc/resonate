import AVFoundation
import ResonateNative
import SwiftUI
import UniformTypeIdentifiers

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
    @State private var confirmingErase = false
    private static let unlistedWords = [
        "Online features internet network artwork covers lyrics enrichment music search",
        "Artwork and details automatically portraits Wi-Fi missing covers",
        "Download providers TIDAL Subsonic Monochrome",
        "Scrobbling ListenBrainz Last.fm",
        "Erase all data reset everything delete start over",
        "Appearance accent colour icon tint theme background shader wallpaper",
    ]

    var body: some View {
        let features = library.features
        let sound = soundEntries.filter { matches($0.searched) }
        let sources = sourceEntries.filter { matches($0.searched) }
        ThemedList {
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
            if shown("Appearance accent colour icon tint theme background shader wallpaper") {
                Section("Appearance") {
                    NavigationLink(value: Destination.appearance) {
                        ToolRow(
                            title: "Accent colour",
                            detail: AppearanceState.shared.accent.map { $0.name.capitalized } ?? "Resonate",
                            symbol: "paintpalette"
                        )
                    }
                    NavigationLink(value: Destination.background) {
                        ToolRow(
                            title: "Background",
                            detail: AppearanceState.shared.themedBackground ? "Resonate" : "Off",
                            symbol: "sparkles.rectangle.stack"
                        )
                    }
                }
            }
            if connectedShown(syncEntry) {
                Section("Connected services") {
                    if shown("Online features", "internet") {
                        Toggle(isOn: Binding(get: { features.online }, set: { features.setOnline($0) })) {
                            ToolRow(title: "Online features", detail: "MusicBrainz, artwork, lyrics, AutoEq and recognition", symbol: "globe")
                        }
                    }
                    if shown("Download providers", "TIDAL", "Subsonic", "Monochrome") {
                        NavigationLink(value: Destination.providers) {
                            ToolRow(title: "Download providers", detail: providersDetail(features.downloadAccounts), symbol: "arrow.down.circle")
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
            if features.online, shown("Artwork and details automatically portraits Wi-Fi missing covers") {
                EnrichmentSection()
            }
            if shown("Erase all data reset everything delete start over") {
                Section {
                    Button(role: .destructive) {
                        confirmingErase = true
                    } label: {
                        Label("Erase all data", systemImage: "trash")
                            .foregroundStyle(.red)
                    }
                    .disabled(library.erasing)
                } footer: {
                    Text("Removes your library, playlists, favourites, listening history, settings, accounts and pairing code from this iPhone, and deletes every song in it: those in Resonate’s own folder and those in folders you added. Other files in those folders stay. Your other devices are not touched.")
                }
            }
        }
        .confirmationDialog("Erase all data?", isPresented: $confirmingErase, titleVisibility: .visible) {
            Button("Erase everything", role: .destructive) { library.eraseEverything() }
        } message: {
            Text("Every song Resonate lists is deleted from this iPhone, including those in folders you added. This cannot be undone. Resonate closes once it is done; open it again to start fresh.")
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
                keywords: "sound playback curve preamp bands profile headphone correction AutoEq EqualizerAPO GraphicEQ import room",
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
                title: "Sleep timer",
                detail: Self.sleepSummary(library.snapshot?.sleep, remaining: nil),
                symbol: "moon",
                keywords: "sound playback pause minutes end song queue",
                opening: .sleepTimer,
                available: library.playingTrack != nil
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
            detail: syncDetail(features),
            symbol: "arrow.triangle.2.circlepath",
            keywords: "connected services devices pairing code phone computer playlists favourites listening history handoff",
            opening: .page(.sync)
        )
    }

    private func syncDetail(_ features: Features) -> String {
        switch features.syncActivity {
        case .fetching(let songs, _): return "Fetching \(counted(songs, "song")) from your devices"
        case .syncing: return "Syncing…"
        case nil: break
        }
        let standing = syncStanding(features.syncStatus, config: features.syncConfig, online: features.online)
        return features.syncUpToDate ? "Up to date · " + standing : standing
    }

    @ViewBuilder
    private func entryRow(_ entry: SettingsEntry) -> some View {
        let row = ToolRow(title: entry.title, detail: entry.detail, symbol: entry.symbol)
        switch entry.opening {
        case .page(let destination):
            NavigationLink(value: destination) { row }
                .disabled(!entry.available)
        case .sleepTimer:
            Button { navigation.sleepTimer = true } label: { SleepTimerRow(entry: entry) }
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

    private func connectedShown(_ syncEntry: SettingsEntry) -> Bool {
        shown("Online features", "internet") || shown("Download providers", "TIDAL", "Subsonic", "Monochrome")
            || shown("Scrobbling", "ListenBrainz", "Last.fm") || matches(syncEntry.searched)
    }

    private func matches(_ searched: String) -> Bool {
        terms.allSatisfy { searched.localizedCaseInsensitiveContains($0) }
    }

    private func shown(_ words: String...) -> Bool {
        matches(words.joined(separator: " "))
    }

    private static func equaliserSummary(_ preferences: PlaybackPreferences?) -> String {
        guard let preferences, preferences.equaliserEnabled else { return "Parametric curve and preamp" }
        return preferences.equaliser.summary
    }

    private static func processingSummary(_ preferences: PlaybackPreferences?) -> String {
        switch preferences?.replayGain {
        case .track: "Track ReplayGain"
        case .album: "Album ReplayGain"
        default: "ReplayGain, resampling and clip prevention"
        }
    }

    static func sleepSummary(_ timer: SleepTimer?, remaining: UInt64?) -> String {
        switch timer {
        case .after: remaining.map { "Pauses in \(clock($0 + 999))" } ?? "Pauses after the set time"
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

private struct SleepTimerRow: View {
    @Environment(Library.self) private var library
    let entry: SettingsEntry

    var body: some View {
        ToolRow(
            title: entry.title,
            detail: SettingsView.sleepSummary(library.snapshot?.sleep, remaining: library.clock.sleepRemainingMs),
            symbol: entry.symbol
        )
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
            CommittingSlider(
                title: title,
                value: value,
                range: range,
                step: 0.5,
                editing: $editing,
                spoken: { String(format: "%+.1f decibels", Double($0)) },
                commit: commit
            )
        }
    }
}

struct SoundSettingsView: View {
    @Environment(Library.self) private var library
    @State private var draft: EqualiserCurve?
    @State private var selected: Int?
    @State private var holding = false
    @State private var response = EqualiserResponse()
    @State private var confirmingReset = false
    @State private var settling: Task<Void, Never>?
    @State private var model = ""
    @State private var pending: PendingProfile?
    @State private var choosingFile = false
    @State private var advanced = false
    private static let limits = equaliserLimits()
    private static let presets = equaliserPresets()
    private static let octaves = tenBandCurve(preampDb: 0, gainsDb: [])
    private static let addedHz: Float = 1000
    private static let settle: Duration = .seconds(1)
    private static let searchSettles: Duration = .milliseconds(350)
    private static let largestProfile = 64 * 1024

    private struct Drawn: Equatable {
        let curve: EqualiserCurve?
        let rate: UInt32
    }

    var body: some View {
        ThemedForm {
            if let settings = library.preferences {
                let curve = draft ?? settings.equaliser
                Section {
                    Toggle("Enable equaliser", isOn: Binding(
                        get: { settings.equaliserEnabled },
                        set: { enabled in library.updatePreferences { var next = $0; next.equaliserEnabled = enabled; return next } }
                    ))
                    LabeledContent("Preset", value: curve.summary)
                    if let route = library.route {
                        Toggle("Only for \(route.name)", isOn: Binding(
                            get: { library.equaliserBound },
                            set: { library.bindEqualiser($0) }
                        ))
                    }
                } footer: {
                    Text(library.equaliserBound
                        ? "This curve is kept for this output and comes back whenever it plays. Other outputs use their own curve or the shared one."
                        : "This curve is shared by every output without a curve of its own.")
                }
                let preset = Self.presets.first { $0.name == curve.presetName }
                Section {
                    EqualiserPresetGrid(presets: Self.presets, chosen: preset?.name, choose: apply)
                } header: {
                    Text("Presets")
                } footer: {
                    Text(preset?.detail ?? "Your own sound. Adjust the sliders to make it yours.")
                }
                Section {
                    Picker("Adjust sound", selection: $advanced) {
                        Text("Sliders").tag(false)
                        Text("Advanced").tag(true)
                    }
                    .pickerStyle(.segmented)
                    .labelsHidden()
                    Button("Reset curve", systemImage: "arrow.counterclockwise", role: .destructive) { confirmingReset = true }
                        .disabled(curve == .flat)
                } header: {
                    Text("Adjust sound")
                } footer: {
                    Text(advanced ? "Select a band below or drag a point on the graph." : "Slide left to reduce, right to boost.")
                }
                if advanced {
                    Section {
                        EqualiserGraph(
                            curve: curve,
                            response: response.decibels,
                            limits: Self.limits,
                            selected: $selected,
                            holding: $holding,
                            edit: { draft = $0 },
                            commit: { edited($0) },
                            add: { add($0, to: curve) },
                            discard: { draft = nil }
                        )
                        .frame(height: 280)
                        .listRowInsets(EdgeInsets(top: 12, leading: 12, bottom: 12, trailing: 12))
                        Button("Add a band", systemImage: "plus") {
                            add(EqualiserBand(filter: .peaking, frequencyHz: Self.addedHz, gainDb: 0, q: EqualiserUnits.addedQ), to: curve)
                        }
                        .disabled(curve.bands.count >= Int(Self.limits.bandsAtMost))
                    } footer: {
                        Text("Tap the graph to add a band. Drag a point to move it, and pinch to widen or narrow the band it holds or the one selected.")
                    }
                    if let selected, curve.bands.indices.contains(selected) {
                        BandInspector(
                            band: curve.bands[selected],
                            number: selected + 1,
                            limits: Self.limits,
                            change: { edited(curve.replacing(selected, with: $0)) },
                            delete: { remove(selected, from: curve) }
                        )
                    }
                } else {
                    let shaped = sliding(curve)
                    Section {
                        EqualiserSliders(
                            curve: shaped,
                            limits: Self.limits,
                            change: { index, gain in slide(index, to: gain, in: shaped) },
                            open: { index in
                                selected = index
                                advanced = true
                            }
                        )
                    }
                }
                Section {
                    LevelSlider(
                        title: "Preamp",
                        value: curve.preampDb,
                        range: -Self.limits.preampAtMostDb...Self.limits.preampAtMostDb
                    ) { gain in
                        var next = curve
                        next.preampDb = gain
                        edited(next)
                    }
                    Button("Fit the preamp", systemImage: "arrow.up.to.line") { fit(curve) }
                        .disabled(curve.bands.isEmpty)
                } header: {
                    Text("Output gain")
                } footer: {
                    Text("Fit the preamp sets it from the curve's own peak, so the loudest part of it reaches full scale and no further.")
                }
                HeadphoneCorrectionSection(model: $model, current: curve, choose: { ask(.device($0)) }, search: search)
                ProfileImportSection(paste: paste, choose: { choosingFile = true })
                RoomCorrectionSection()
                let others = library.boundRoutes.filter { $0.id != library.route?.id }
                if !others.isEmpty {
                    Section {
                        ForEach(others, id: \.id) { route in
                            Label(route.name, systemImage: "hifispeaker")
                                .swipeActions {
                                    Button("Forget", role: .destructive) { library.forgetEqualiser(for: route) }
                                }
                                .contextMenu {
                                    Button("Forget its curve", systemImage: "trash", role: .destructive) {
                                        library.forgetEqualiser(for: route)
                                    }
                                }
                        }
                    } header: {
                        Text("Curves kept for other outputs")
                    } footer: {
                        Text("Each comes back when that output plays. Forgetting one lets it use the shared curve.")
                    }
                }
            }
        }
        .navigationTitle("Equaliser")
        .onChange(of: drawn, initial: true) { _, drawn in
            guard let curve = drawn.curve else { return }
            response.follow(curve, rate: drawn.rate > 0 ? drawn.rate : nil)
        }
        .onChange(of: library.preferences?.equaliser) { _, committed in
            if draft == committed || !holding { draft = nil }
            if let selected, let committed, !committed.bands.indices.contains(selected) { self.selected = nil }
        }
        .confirmationDialog("Reset the curve?", isPresented: $confirmingReset, titleVisibility: .visible) {
            Button("Remove every band", role: .destructive) { replace(with: .flat) }
            Button("Start from ten flat bands", role: .destructive) { replace(with: tenBandCurve(preampDb: 0, gainsDb: [])) }
        } message: {
            Text("The preamp returns to 0 dB and the current preset is cleared.")
        }
        .task(id: model) {
            let text = model.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !text.isEmpty else {
                library.features.clearCorrections()
                return
            }
            try? await Task.sleep(for: Self.searchSettles)
            guard !Task.isCancelled else { return }
            search(text)
        }
        .confirmationDialog(
            "Replace the current curve?",
            isPresented: Binding(get: { pending != nil }, set: { if !$0 { pending = nil } }),
            titleVisibility: .visible,
            presenting: pending
        ) { chosen in
            Button("Load \(chosen.name)") { load(chosen) }
        } message: { chosen in
            Text("\(chosen.name) replaces the \(counted(library.preferences?.equaliser.bands.count ?? 0, "band")) you have now.")
        }
        .fileImporter(isPresented: $choosingFile, allowedContentTypes: [.plainText, .text, .data]) { result in
            switch result {
            case .success(let url): read(url)
            case .failure(let failure): library.report(failure)
            }
        }
    }

    private var drawn: Drawn {
        Drawn(curve: draft ?? library.preferences?.equaliser, rate: library.clock.outputRate)
    }

    private func commit(_ curve: EqualiserCurve, switchingOn: Bool = false) {
        draft = curve
        settling?.cancel()
        settling = Task {
            try? await Task.sleep(for: Self.settle)
            guard !Task.isCancelled, !holding, draft == curve else { return }
            draft = nil
        }
        library.updatePreferences { current in
            var next = current
            next.equaliser = curve
            if switchingOn { next.equaliserEnabled = true }
            return next
        }
    }

    private func fit(_ curve: EqualiserCurve) {
        let rate = library.clock.outputRate
        let asked: UInt32? = rate > 0 ? rate : nil
        Task {
            do {
                let fitted = try await Work.aside { try fittedPreamp(curve: curve, rateHz: asked) }
                var next = curve
                next.preampDb = fitted
                edited(next)
            } catch {
                library.report(error)
            }
        }
    }

    private func apply(_ preset: EqualiserPreset) {
        selected = nil
        commit(preset.curve, switchingOn: true)
    }

    private func edited(_ curve: EqualiserCurve, switchingOn: Bool = false) {
        var next = curve
        next.name = ""
        commit(next, switchingOn: switchingOn)
    }

    private func sliding(_ curve: EqualiserCurve) -> EqualiserCurve {
        guard curve.bands.isEmpty else { return curve }
        var octaves = Self.octaves
        octaves.preampDb = curve.preampDb
        return octaves
    }

    private func slide(_ index: Int, to gain: Float, in curve: EqualiserCurve) {
        guard curve.bands.indices.contains(index) else { return }
        var band = curve.bands[index]
        band.gainDb = gain
        edited(curve.replacing(index, with: band), switchingOn: true)
    }

    private func add(_ band: EqualiserBand, to curve: EqualiserCurve) {
        guard curve.bands.count < Int(Self.limits.bandsAtMost) else { return }
        var next = curve
        next.bands.append(band)
        selected = next.bands.count - 1
        edited(next, switchingOn: true)
    }

    private func remove(_ index: Int, from curve: EqualiserCurve) {
        guard curve.bands.indices.contains(index) else { return }
        var next = curve
        next.bands.remove(at: index)
        selected = nil
        edited(next)
    }

    private func replace(with curve: EqualiserCurve) {
        selected = nil
        commit(curve)
    }

    private func search(_ text: String) {
        let text = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard library.features.online, !text.isEmpty else { return }
        library.features.searchCorrections(text)
    }

    private func ask(_ chosen: PendingProfile) {
        if library.preferences?.equaliser.bands.isEmpty ?? true {
            load(chosen)
        } else {
            pending = chosen
        }
    }

    private func load(_ chosen: PendingProfile) {
        pending = nil
        selected = nil
        switch chosen {
        case .device(let device): library.features.applyCorrection(device)
        case .text(let name, let bytes): library.features.applyProfile(name: name, bytes: bytes)
        }
    }

    private func paste() {
        guard let text = UIPasteboard.general.string, !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            library.notice("The clipboard holds no text")
            return
        }
        ask(.text(name: "Pasted profile", bytes: Data(text.utf8)))
    }

    private func read(_ url: URL) {
        let name = url.deletingPathExtension().lastPathComponent
        let largest = Self.largestProfile
        Task {
            do {
                let bytes = try await Work.aside { try Self.contents(of: url, atMost: largest) }
                ask(.text(name: name, bytes: bytes))
            } catch {
                library.report(error)
            }
        }
    }

    nonisolated private static func contents(of url: URL, atMost largest: Int) throws -> Data {
        let scoped = url.startAccessingSecurityScopedResource()
        defer { if scoped { url.stopAccessingSecurityScopedResource() } }
        let handle = try FileHandle(forReadingFrom: url)
        defer { try? handle.close() }
        return try handle.read(upToCount: largest + 1) ?? Data()
    }
}

enum PendingProfile {
    case device(AutoEqDevice)
    case text(name: String, bytes: Data)

    var name: String {
        switch self {
        case .device(let device): device.name
        case .text(let name, _): name
        }
    }
}

struct HeadphoneCorrectionSection: View {
    @Environment(Library.self) private var library
    @Binding var model: String
    let current: EqualiserCurve
    let choose: (AutoEqDevice) -> Void
    let search: (String) -> Void

    var body: some View {
        let features = library.features
        Section {
            TextField("Search headphone models", text: $model)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
                .submitLabel(.search)
                .onSubmit { search(model) }
            if !features.online {
                Text("Turn on online features in Settings to find corrections.").foregroundStyle(.secondary)
            } else if features.searchingCorrections {
                HStack(spacing: 12) {
                    ProgressView()
                    Text("Searching the AutoEq catalogue…").foregroundStyle(.secondary)
                }
            } else if let searched = features.correctionsFor, features.corrections.isEmpty {
                Text("No headphones match “\(searched)”. Try the model name as printed on them.").foregroundStyle(.secondary)
            }
            ForEach(features.corrections, id: \.id) { device in
                Button {
                    choose(device)
                } label: {
                    HStack {
                        Label(device.name, systemImage: "headphones").foregroundStyle(Color.primary)
                        Spacer()
                        if current.name == device.name {
                            Image(systemName: "checkmark")
                                .foregroundStyle(.tint)
                                .accessibilityLabel("Current preset")
                        }
                    }
                }
                .disabled(features.applyingCorrection)
            }
        } header: {
            Text("Headphone correction")
        } footer: {
            Text("An AutoEq correction loads as the bands above, ready to adjust on the graph.")
        }
    }
}

struct ProfileImportSection: View {
    let paste: () -> Void
    let choose: () -> Void

    var body: some View {
        Section {
            Button("Paste from the clipboard", systemImage: "doc.on.clipboard", action: paste)
            Button("Import a text file", systemImage: "doc.text", action: choose)
        } header: {
            Text("Import a profile")
        } footer: {
            Text("EqualizerAPO or GraphicEQ text. Parametric filters load as bands; a GraphicEQ line is fitted onto bands.")
        }
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
