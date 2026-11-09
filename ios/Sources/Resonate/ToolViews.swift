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
            .navigationTitle(track?.shownTitle ?? said("lyrics.title"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                if let track {
                    Menu {
                        if features.online {
                            Button("lyrics.refresh", systemImage: "arrow.clockwise") { features.loadLyrics(track.id, refresh: true) }
                        }
                        Button("lyrics.add", systemImage: "text.badge.plus") {
                            writing = LyricWriting(track: track)
                        }
                    } label: {
                        Label("lyrics.options", systemImage: "ellipsis")
                    }
                }
            }
            .sheet(item: $writing) { LyricWriter(track: $0.track).sheetSized(.long) }
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
    @State private var discarding = false
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
                        .accessibilityLabel("lyrics.title")
                    PasteButton(payloadType: String.self) { pasted in
                        guard let first = pasted.first else { return }
                        Task { @MainActor in text = first }
                    }
                } header: {
                    Text(track.shownTitle)
                } footer: {
                    Text("lyrics.writer.footer")
                }
            }
            .navigationTitle("lyrics.title")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("common.cancel") {
                        if blank { dismiss() } else { discarding = true }
                    }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("common.save") {
                        library.features.keepLyricText(track.id, text)
                        dismiss()
                    }
                    .disabled(blank)
                }
            }
        }
        .keepingEditsUnlessDiscarded(!blank, asking: $discarding) { dismiss() }
    }
}

extension View {
    func keepingEditsUnlessDiscarded(
        _ edited: Bool,
        asking: Binding<Bool>,
        discard: @escaping () -> Void
    ) -> some View {
        interactiveDismissDisabled(edited)
            .confirmationDialog("edits.discard.question", isPresented: asking, titleVisibility: .visible) {
                Button("edits.discard", role: .destructive, action: discard)
                Button("edits.keep", role: .cancel) {}
            }
    }
}

struct SignalPlot: View {
    let values: [Float]
    var mirrored = false
    var fittedToPeak = true
    let label: String
    var spoken: String?

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
        .accessibilityElement()
        .accessibilityLabel(label)
        .accessibilityValue(spoken ?? "")
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
                    "analysis.none.title",
                    systemImage: "waveform.path.ecg",
                    description: Text("analysis.none.description")
                )
            }
        }
        .navigationTitle("analysis.title")
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
                ProgressView(value: progress) { Text("analysis.analysing") }
            } else {
                HStack(spacing: 12) {
                    ProgressView().accessibilityHidden(true)
                    Text("analysis.analysing")
                }
            }
            Button("analysis.cancel", role: .destructive) { features.cancelAnalysis() }
        } else {
            Button(said(analysed ? "analysis.again" : "analysis.analyse")) { features.analyse(track.id) }
        }
    }
}

struct AnalysisReportSections: View {
    let report: AnalysisReport
    private static let spectrumFloorDb: Float = 120

    var body: some View {
        Section("analysis.source") {
            LabeledContent("analysis.assessment", value: report.verdict)
            ForEach(report.findings, id: \.self) { Text($0).font(.footnote) }
        }
        Section("analysis.waveform") {
            SignalPlot(
                values: report.waveform,
                mirrored: true,
                label: said("analysis.waveform"),
                spoken: said("analysis.waveform.spoken", Self.dbfs(report.peak), report.verdict)
            )
            .frame(height: 120)
        }
        Section("analysis.spectrum") {
            SignalPlot(
                values: Self.spectrumHeights(report.spectrum),
                fittedToPeak: false,
                label: said("analysis.spectrum"),
                spoken: said("analysis.spectrum.spoken", report.verdict)
            )
            .frame(height: 120)
            if let last = report.spectrum.indices.last {
                LabeledContent("analysis.range", value: said("analysis.range.value", String(format: "%.1f", Double(report.spectrumStepHz) * Double(last) / 1000)))
            }
        }
        Section("analysis.levels") {
            LabeledContent(
                "analysis.loudness",
                value: report.loudnessLufs.map { said("analysis.loudness.value", String(format: "%.1f", $0)) } ?? said("analysis.not-measurable")
            )
            LabeledContent("analysis.dynamic-range", value: report.dynamicRange.map { "DR\($0)" } ?? said("analysis.not-measurable"))
            LabeledContent("analysis.peak", value: Self.dbfs(report.peak))
            LabeledContent("analysis.true-peak", value: Self.dbtp(report.truePeak))
            LabeledContent("analysis.rms", value: Self.dbfs(report.rms))
            LabeledContent("analysis.clipped", value: "\(report.clipped)")
            LabeledContent("analysis.bits-in-use", value: report.bitsInUse.map { "\($0)" } ?? said("common.unknown"))
        }
    }

    private static func dbfs(_ linear: Float) -> String {
        decibels(linear).map { said("analysis.level.dbfs", $0) } ?? said("analysis.level.silent")
    }

    private static func dbtp(_ linear: Float) -> String {
        decibels(linear).map { said("analysis.level.dbtp", $0) } ?? said("analysis.level.silent")
    }

    private static func decibels(_ linear: Float) -> String? {
        guard linear > 0, linear.isFinite else { return nil }
        return String(format: "%.2f", 20 * log10(Double(linear)))
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
                Text("recognition.offline").foregroundStyle(.secondary)
            } else {
                SecureField("recognition.key", text: $key)
                Button("recognition.key.save") { features.saveRecognitionKey(key) }
                    .disabled(key == features.recognitionKey)
                Button(said(features.recognitionBusy ? "recognition.identifying" : "recognition.identify")) { features.recognise(track.id) }
                    .disabled(features.recognitionBusy)
                ForEach(features.recognition, id: \.song.recording) { candidate in
                    RecognitionRow(candidate: candidate)
                }
            }
        } header: {
            Text("recognition.title")
        } footer: {
            Text("recognition.footer")
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
            Text(said("recognition.match", Int(candidate.score))).font(.caption).foregroundStyle(.tint)
            if let page = URL(string: "https://musicbrainz.org/recording/\(candidate.song.recording)") {
                Link("recognition.view", destination: page).font(.subheadline)
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
        .navigationTitle("visualizer.title")
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
            ContentUnavailableView("visualizer.idle", systemImage: "waveform")
        } else {
            SignalPlot(
                values: samples,
                mirrored: true,
                label: said("visualizer.title"),
                spoken: said(library.transport == .playing ? "visualizer.playing" : "visualizer.paused")
            )
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
                Button("inspector.add-to-playlist", systemImage: "text.badge.plus") { choosingPlaylist = true }
                if features.tagsWritable(shown) {
                    Button("inspector.edit-tags", systemImage: "pencil") { navigation.editTags([shown]) }
                }
                Button("inspector.lyrics", systemImage: "quote.bubble") { navigation.open(.lyrics(shown)) }
                Button("inspector.analyse", systemImage: "waveform.path.ecg") { navigation.open(.analysis(shown)) }
                if let albumPage = shown.albumPage {
                    Button("inspector.go-to-album", systemImage: "square.stack") { navigation.open(albumPage) }
                }
                if !shown.artist.isEmpty {
                    Button("inspector.go-to-artist", systemImage: "music.mic") { navigation.open(.artist(shown.artist)) }
                }
                if features.online {
                    Button(said(features.enriching ? "inspector.enriching" : "inspector.enrich"), systemImage: "sparkles") {
                        features.enrich(shown)
                    }
                    .disabled(features.enriching)
                    Button("inspector.artist-details", systemImage: "music.mic") { features.loadArtist(shown.id) }
                }
            }
            InspectorTags(track: shown, inspector: inspector)
            InspectorAudio(track: shown, inspector: inspector)
            Section("inspector.document") {
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
        .sheet(isPresented: $choosingPlaylist) { PlaylistPicker(tracks: [shown]).sheetSized() }
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
        Section("inspector.tags") {
            LabeledContent("inspector.album", value: Self.tagged(track.album))
            LabeledContent("inspector.album-artist", value: Self.tagged(track.albumArtist))
            LabeledContent("inspector.genre", value: Self.tagged(track.genre))
            LabeledContent("inspector.date", value: Self.tagged(track.date))
            LabeledContent("inspector.track", value: track.trackNumber.map { "\($0)" } ?? said("inspector.not-tagged"))
            LabeledContent("inspector.disc", value: track.discNumber.map { "\($0)" } ?? said("inspector.not-tagged"))
            identifier(said("inspector.recording-id"), track.recording)
            identifier(said("inspector.release-id"), track.release)
            if let inspector {
                LabeledContent("ISRC", value: Self.tagged(inspector.isrc))
                if !inspector.copyright.isEmpty { LabeledContent("inspector.copyright", value: inspector.copyright) }
                if !inspector.comment.isEmpty { LabeledContent("inspector.comment", value: inspector.comment) }
            }
        }
    }

    static func tagged(_ value: String) -> String {
        value.isEmpty ? said("inspector.not-tagged") : value
    }

    private func identifier(_ title: String, _ value: String?) -> some View {
        LabeledContent(title) {
            Text(value ?? said("inspector.not-identified"))
                .font(value == nil ? .body : .footnote.monospaced())
                .textSelection(.enabled)
        }
    }
}

struct InspectorAudio: View {
    let track: Track
    let inspector: Inspector?

    var body: some View {
        Section("inspector.audio") {
            LabeledContent("inspector.codec", value: track.codec)
            LabeledContent("inspector.sample-rate", value: said("inspector.sample-rate.value", Int(track.rate)))
            LabeledContent("inspector.bit-depth", value: track.bits.map { said("inspector.bit-depth.value", Int($0)) } ?? said("common.unknown"))
            LabeledContent("inspector.channels", value: "\(track.channels)")
            LabeledContent("inspector.duration", value: clock(track.durationMs))
            if let inspector {
                LabeledContent("inspector.container", value: inspector.container)
                LabeledContent("inspector.decoded-format", value: inspector.sampleFormat)
                LabeledContent("inspector.seekable", value: said(inspector.seekable ? "common.yes" : "common.no"))
                LabeledContent("inspector.encoder", value: InspectorTags.tagged(inspector.encoder))
            } else {
                HStack(spacing: 12) {
                    ProgressView().accessibilityHidden(true)
                    Text("inspector.reading").foregroundStyle(.secondary)
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
                    ProgressView().accessibilityHidden(true)
                    Text("inspector.artist.looking").foregroundStyle(.secondary)
                }
            }
        case .found(let artist):
            ArtistInformationSection(artist: artist)
        case .missing:
            Section {
                ContentUnavailableView(
                    "inspector.artist.none.title",
                    systemImage: "music.mic",
                    description: Text("inspector.artist.none.description")
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
            if !artist.kind.isEmpty { LabeledContent("inspector.artist.kind", value: artist.kind) }
            if !artist.country.isEmpty { LabeledContent("inspector.artist.country", value: artist.country) }
            if !artist.area.isEmpty { LabeledContent("inspector.artist.area", value: artist.area) }
            if !artist.genres.isEmpty { LabeledContent("inspector.artist.genres", value: artist.genres.joined(separator: ", ")) }
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
