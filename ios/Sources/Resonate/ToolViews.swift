import ResonateLogic
import ResonateNative
import SwiftUI

struct LyricsView: View {
    @Environment(Library.self) private var library
    @State private var writing: LyricWriting?
    let chosen: Track?

    var body: some View {
        let features = library.features
        let track = chosen ?? library.playingTrack
        let sheet = features.lyrics?.trackId == track?.id ? features.lyrics : nil
        LyricsPane(track: track, compact: false, addLyrics: track.map { song in { writing = LyricWriting(track: song) } })
            .safeAreaInset(edge: .bottom) {
                if let sheet, !sheet.lines.isEmpty {
                    LyricAttribution(sheet: sheet)
                }
            }
            .background(BackdropView(track: track))
            .navigationTitle(track?.shownTitle ?? "Lyrics")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                if let track {
                    Menu {
                        if features.online {
                            Button("Refresh", systemImage: "arrow.clockwise") { features.loadLyrics(track.id, refresh: true) }
                        }
                        Button("Add lyrics…", systemImage: "text.badge.plus") {
                            writing = LyricWriting(track: track)
                        }
                    } label: {
                        Label("Lyrics options", systemImage: "ellipsis")
                    }
                }
            }
            .sheet(item: $writing) { LyricWriter(track: $0.track) }
    }
}

struct LyricWriting: Identifiable {
    let track: Track
    var id: UInt64 { track.id }
}

struct LyricWriter: View {
    @Environment(Library.self) private var library
    @Environment(\.dismiss) private var dismiss
    @State private var text = ""
    let track: Track

    var body: some View {
        let blank = text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        NavigationStack {
            Form {
                Section {
                    TextEditor(text: $text)
                        .font(.body.monospaced())
                        .frame(minHeight: 240)
                        .autocorrectionDisabled()
                        .accessibilityLabel("Lyrics")
                    PasteButton(payloadType: String.self) { pasted in
                        guard let first = pasted.first else { return }
                        Task { @MainActor in text = first }
                    }
                } header: {
                    Text(track.shownTitle)
                } footer: {
                    Text("Type or paste the words. Lines timed as LRC, such as [01:23.45], are followed as the song plays; plain lines are shown as they are.")
                }
            }
            .navigationTitle("Lyrics")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Save") {
                        library.features.keepLyricText(track.id, text)
                        dismiss()
                    }
                    .disabled(blank)
                }
            }
        }
        .interactiveDismissDisabled(!blank)
    }
}

extension View {
    func keepingEditsUnlessDiscarded(
        _ edited: Bool,
        asking: Binding<Bool>,
        discard: @escaping () -> Void
    ) -> some View {
        interactiveDismissDisabled(edited)
            .confirmationDialog("Discard your changes?", isPresented: asking, titleVisibility: .visible) {
                Button("Discard changes", role: .destructive, action: discard)
                Button("Keep editing", role: .cancel) {}
            }
    }
}

struct SignalPlot: View {
    let values: [Float]
    var mirrored = false
    var fittedToPeak = true

    var body: some View {
        Canvas { context, size in
            let levels = values.map { $0.isFinite ? CGFloat(abs($0)) : 0 }
            guard levels.count > 1 else { return }
            let peak = levels.max() ?? 1
            let scale = fittedToPeak && peak > 0 ? 1 / peak : 1
            let step = size.width / CGFloat(levels.count - 1)
            var path = Path()
            for (index, raw) in levels.enumerated() {
                let x = CGFloat(index) * step
                let level = min(raw * scale, 1)
                if mirrored {
                    path.move(to: CGPoint(x: x, y: size.height / 2 * (1 - level)))
                    path.addLine(to: CGPoint(x: x, y: size.height / 2 * (1 + level)))
                } else if index == 0 {
                    path.move(to: CGPoint(x: x, y: size.height * (1 - level)))
                } else {
                    path.addLine(to: CGPoint(x: x, y: size.height * (1 - level)))
                }
            }
            context.stroke(path, with: .color(Color.worn), lineWidth: mirrored ? max(step * 0.7, 1) : 1.5)
        }
        .accessibilityHidden(true)
    }
}

struct AnalysisView: View {
    @Environment(Library.self) private var library
    let chosen: Track?

    var body: some View {
        let features = library.features
        let track = chosen ?? library.playingTrack
        let report = features.analysis?.trackId == track?.id ? features.analysis : nil
        Group {
            if let track {
                ThemedList {
                    Section { CollectionRow(track: track, title: track.shownTitle, detail: track.quality) }
                    Section { AnalysisProgress(track: track, analysed: report != nil) }
                    if let report {
                        AnalysisReportSections(report: report)
                        RecognitionSection(track: track)
                    }
                }
            } else {
                ContentUnavailableView(
                    "Choose a song to analyse",
                    systemImage: "waveform.path.ecg",
                    description: Text("Play a song, or open Analyse from a song's details.")
                )
            }
        }
        .navigationTitle("Analysis")
        .navigationBarTitleDisplayMode(.inline)
        .task(id: track?.id) {
            guard let track, features.analysis?.trackId != track.id else { return }
            features.analyse(track.id)
        }
        .onDisappear { features.cancelAnalysis() }
    }
}

struct AnalysisProgress: View {
    @Environment(Library.self) private var library
    let track: Track
    let analysed: Bool

    var body: some View {
        let features = library.features
        if features.analysisBusy {
            if let progress = features.analysisProgress {
                ProgressView(value: progress) { Text("Analysing audio…") }
            } else {
                HStack(spacing: 12) {
                    ProgressView()
                    Text("Analysing audio…")
                }
            }
            Button("Cancel analysis", role: .destructive) { features.cancelAnalysis() }
        } else {
            Button(analysed ? "Analyse again" : "Analyse") { features.analyse(track.id) }
        }
    }
}

struct AnalysisReportSections: View {
    let report: AnalysisReport
    private static let spectrumFloorDb: Float = 120

    var body: some View {
        Section("Source audio") {
            LabeledContent("Assessment", value: report.verdict)
            ForEach(report.findings, id: \.self) { Text($0).font(.footnote) }
        }
        Section("Waveform") {
            SignalPlot(values: report.waveform, mirrored: true).frame(height: 120)
        }
        Section("Spectrum") {
            SignalPlot(values: Self.spectrumHeights(report.spectrum), fittedToPeak: false).frame(height: 120)
            if let last = report.spectrum.indices.last {
                LabeledContent("Range", value: String(format: "0 – %.1f kHz", Double(report.spectrumStepHz) * Double(last) / 1000))
            }
        }
        Section("Levels") {
            LabeledContent("Integrated loudness", value: report.loudnessLufs.map { String(format: "%.1f LUFS", $0) } ?? "Not measurable")
            LabeledContent("Dynamic range", value: report.dynamicRange.map { "DR\($0)" } ?? "Not measurable")
            LabeledContent("Peak", value: Self.level(report.peak, unit: "dBFS"))
            LabeledContent("True peak", value: Self.level(report.truePeak, unit: "dBTP"))
            LabeledContent("RMS", value: Self.level(report.rms, unit: "dBFS"))
            LabeledContent("Clipped samples", value: "\(report.clipped)")
            LabeledContent("Bits in use", value: report.bitsInUse.map { "\($0)" } ?? "Unknown")
        }
    }

    private static func level(_ linear: Float, unit: String) -> String {
        guard linear > 0, linear.isFinite else { return "Silent" }
        return String(format: "%.2f", 20 * log10(Double(linear))) + " " + unit
    }

    private static func spectrumHeights(_ levels: [Float]) -> [Float] {
        levels.map { decibels in
            guard decibels.isFinite else { return 0 }
            return min(max((decibels + spectrumFloorDb) / spectrumFloorDb, 0), 1)
        }
    }
}

struct RecognitionSection: View {
    @Environment(Library.self) private var library
    let track: Track
    @State private var key = ""

    var body: some View {
        let features = library.features
        Section {
            if !features.online {
                Text("Enable online features in Settings to identify recordings.").foregroundStyle(.secondary)
            } else {
                SecureField("AcoustID key (optional)", text: $key)
                Button("Save recognition key") { features.saveRecognitionKey(key) }
                    .disabled(key == features.recognitionKey)
                Button(features.recognitionBusy ? "Identifying…" : "Identify by sound") { features.recognise(track.id) }
                    .disabled(features.recognitionBusy)
                ForEach(features.recognition, id: \.song.recording) { candidate in
                    RecognitionRow(candidate: candidate)
                }
            }
        } header: {
            Text("Identify this recording")
        } footer: {
            Text("Recognition sends an audio signature to Shazam. With your AcoustID key it sends the whole-song fingerprint to AcoustID instead, and the library is studied and recognised in the background while online lookups run, which fills the misnamed search.")
        }
        .onChange(of: features.recognitionKey, initial: true) { _, saved in key = saved }
    }
}

struct RecognitionRow: View {
    let candidate: RecognitionCandidate

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(candidate.song.title).font(.body.weight(.medium))
            Text("\(candidate.song.artist) · \(candidate.song.album)").font(.subheadline).foregroundStyle(.secondary)
            Text("\(candidate.score)% match").font(.caption).foregroundStyle(.tint)
            if let page = URL(string: "https://musicbrainz.org/recording/\(candidate.song.recording)") {
                Link("View recording", destination: page).font(.subheadline)
            }
        }
    }
}

struct VisualizerView: View {
    @Environment(Library.self) private var library

    var body: some View {
        VStack(spacing: 20) {
            LiveSignal()
            if let track = library.playingTrack {
                Text(track.shownTitle).font(.headline)
                Text(track.quality).font(.subheadline).foregroundStyle(.secondary)
            }
        }
        .padding()
        .background(BackdropView(track: library.playingTrack))
        .navigationTitle("Live waveform")
        .navigationBarTitleDisplayMode(.inline)
        .onAppear { library.features.visualize(true) }
        .onDisappear { library.features.visualize(false) }
    }
}

struct LiveSignal: View {
    @Environment(Library.self) private var library

    var body: some View {
        let samples = library.features.samples
        if library.transport != .playing, samples.isEmpty {
            ContentUnavailableView("Play a song to see its sound", systemImage: "waveform")
        } else {
            SignalPlot(values: samples, mirrored: true)
                .frame(maxWidth: .infinity, maxHeight: 320)
                .padding()
                .glassEffect(.regular, in: .rect(cornerRadius: 24))
        }
    }
}

struct InspectorView: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    let track: Track
    @State private var choosingPlaylist = false

    var body: some View {
        let features = library.features
        let inspector = features.inspector?.track.id == track.id ? features.inspector : nil
        let shown = inspector?.track ?? track
        ThemedList {
            InspectorCover(track: shown)
            Section {
                Button("Add to playlist", systemImage: "text.badge.plus") { choosingPlaylist = true }
                if features.tagsWritable(shown) {
                    Button("Edit tags", systemImage: "pencil") { navigation.editTags([shown]) }
                }
                Button("Lyrics", systemImage: "quote.bubble") { navigation.open(.lyrics(shown)) }
                Button("Analyse", systemImage: "waveform.path.ecg") { navigation.open(.analysis(shown)) }
                if let albumPage = shown.albumPage {
                    Button("Go to album", systemImage: "square.stack") { navigation.open(albumPage) }
                }
                if !shown.artist.isEmpty {
                    Button("Go to artist", systemImage: "music.mic") { navigation.open(.artist(shown.artist)) }
                }
                if features.online {
                    Button(features.enriching ? "Enriching…" : "Enrich metadata, artwork and lyrics", systemImage: "sparkles") {
                        features.enrich(shown)
                    }
                    .disabled(features.enriching)
                    Button("Artist details", systemImage: "music.mic") { features.loadArtist(shown.id) }
                }
            }
            InspectorTags(track: shown, inspector: inspector)
            InspectorAudio(track: shown, inspector: inspector)
            Section("Document") {
                Text(shown.uri)
                    .font(.footnote.monospaced())
                    .textSelection(.enabled)
            }
            if features.inspectorTrack == track.id {
                InspectorArtist(lookup: features.inspectorArtist)
            }
        }
        .navigationTitle(shown.shownTitle)
        .navigationBarTitleDisplayMode(.inline)
        .task(id: track.id) { features.loadInspector(track.id) }
        .onChange(of: features.enriching) { wasEnriching, enriching in
            if wasEnriching, !enriching { features.loadInspector(track.id) }
        }
        .sheet(isPresented: $choosingPlaylist) { PlaylistPicker(tracks: [shown]) }
    }
}

struct InspectorCover: View {
    let track: Track
    private static let largestEdge: CGFloat = 280

    var body: some View {
        Section {
            VStack(spacing: 12) {
                ArtworkView(track: track, corner: 20)
                    .aspectRatio(1, contentMode: .fit)
                    .frame(maxWidth: Self.largestEdge)
                VStack(spacing: 4) {
                    Text(track.shownTitle).font(.title3.weight(.semibold))
                    Text(track.shownArtist).foregroundStyle(.secondary)
                }
                .multilineTextAlignment(.center)
            }
            .frame(maxWidth: .infinity)
            .listRowBackground(Color.clear)
        }
    }
}

struct InspectorTags: View {
    let track: Track
    let inspector: Inspector?

    var body: some View {
        Section("Tags") {
            LabeledContent("Album", value: Self.tagged(track.album))
            LabeledContent("Album artist", value: Self.tagged(track.albumArtist))
            LabeledContent("Genre", value: Self.tagged(track.genre))
            LabeledContent("Date", value: Self.tagged(track.date))
            LabeledContent("Track", value: track.trackNumber.map { "\($0)" } ?? "Not tagged")
            LabeledContent("Disc", value: track.discNumber.map { "\($0)" } ?? "Not tagged")
            identifier("Recording ID", track.recording)
            identifier("Release ID", track.release)
            if let inspector {
                LabeledContent("ISRC", value: Self.tagged(inspector.isrc))
                if !inspector.copyright.isEmpty { LabeledContent("Copyright", value: inspector.copyright) }
                if !inspector.comment.isEmpty { LabeledContent("Comment", value: inspector.comment) }
            }
        }
    }

    static func tagged(_ value: String) -> String {
        value.isEmpty ? "Not tagged" : value
    }

    private func identifier(_ title: String, _ value: String?) -> some View {
        LabeledContent(title) {
            Text(value ?? "Not identified")
                .font(value == nil ? .body : .footnote.monospaced())
                .textSelection(.enabled)
        }
    }
}

struct InspectorAudio: View {
    let track: Track
    let inspector: Inspector?

    var body: some View {
        Section("Audio") {
            LabeledContent("Codec", value: track.codec)
            LabeledContent("Sample rate", value: "\(track.rate) Hz")
            LabeledContent("Bit depth", value: track.bits.map { "\($0)-bit" } ?? "Unknown")
            LabeledContent("Channels", value: "\(track.channels)")
            LabeledContent("Duration", value: clock(track.durationMs))
            if let inspector {
                LabeledContent("Container", value: inspector.container)
                LabeledContent("Decoded format", value: inspector.sampleFormat)
                LabeledContent("Seekable", value: inspector.seekable ? "Yes" : "No")
                LabeledContent("Encoder", value: InspectorTags.tagged(inspector.encoder))
            } else {
                HStack(spacing: 12) {
                    ProgressView()
                    Text("Reading the file…").foregroundStyle(.secondary)
                }
            }
        }
    }
}

struct InspectorArtist: View {
    let lookup: ArtistLookup

    var body: some View {
        switch lookup {
        case .idle:
            EmptyView()
        case .looking:
            Section {
                HStack(spacing: 12) {
                    ProgressView()
                    Text("Looking up the artist…").foregroundStyle(.secondary)
                }
            }
        case .found(let artist):
            ArtistInformationSection(artist: artist)
        case .missing:
            Section {
                ContentUnavailableView(
                    "No artist details found",
                    systemImage: "music.mic",
                    description: Text("MusicBrainz knows nothing more about this artist.")
                )
            }
        }
    }
}

struct ArtistInformationSection: View {
    private static let portraitEdge = ArtworkBucket.page.rawValue

    let artist: ArtistInformation
    @State private var portrait: UIImage?

    var body: some View {
        Section(artist.name) {
            if let portrait {
                Image(uiImage: portrait)
                    .resizable()
                    .scaledToFit()
                    .frame(maxHeight: 220)
                    .clipShape(.rect(cornerRadius: 16))
            }
            if !artist.kind.isEmpty { LabeledContent("Kind", value: artist.kind) }
            if !artist.country.isEmpty { LabeledContent("Country", value: artist.country) }
            if !artist.area.isEmpty { LabeledContent("Area", value: artist.area) }
            if !artist.genres.isEmpty { LabeledContent("Genres", value: artist.genres.joined(separator: ", ")) }
            ForEach(artist.links, id: \.self) { link in
                if let url = URL(string: link) {
                    Link(url.host() ?? link, destination: url)
                }
            }
        }
        .task(id: artist.picture) {
            guard let bytes = artist.picture else {
                portrait = nil
                return
            }
            let edge = Self.portraitEdge
            portrait = try? await Work.aside { ArtworkDecoder.thumbnail(bytes, edge: edge) }
        }
    }
}
