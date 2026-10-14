import ResonateNative
import SwiftUI

struct LyricsView: View {
    @Environment(Library.self) private var library
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    let chosen: Track?
    private static let nearby = 3

    var body: some View {
        let features = library.features
        let track = chosen ?? library.snapshot?.current
        let sheet = features.lyrics?.trackId == track?.id ? features.lyrics : nil
        let lines = sheet?.lines ?? []
        let active = features.lyricFrame?.line.map(Int.init)
        let lit = Set((features.lyricFrame?.active ?? []).map(Int.init))
        let position = library.snapshot?.positionMs ?? 0
        ScrollViewReader { scroller in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 28) {
                    if features.lyricsLoading {
                        ProgressView().frame(maxWidth: .infinity)
                    }
                    if lines.isEmpty, !features.lyricsLoading {
                        Text(features.online
                            ? "No lyrics found for this song"
                            : "No saved lyrics. Enable online features to look them up, or add an LRC beside the song.")
                            .foregroundStyle(.secondary)
                            .padding(32)
                    }
                    ForEach(Array(lines.enumerated()), id: \.offset) { index, line in
                        let timed = line.atMs != nil
                        let shining = !timed || lit.contains(index)
                        let distance = active.map { abs(index - $0) } ?? 0
                        LyricLine(line: line, shining: shining, timed: timed, position: position)
                            .scaleEffect(shining || reduceMotion ? 1 : 0.92, anchor: .leading)
                            .opacity(shining ? 1 : (active == nil || distance < Self.nearby ? 0.55 : 0.3))
                            .blur(radius: timed && active != nil && distance > Self.nearby && !reduceMotion ? 1 : 0)
                            .animation(.spring, value: shining)
                            .id(index)
                            .onTapGesture {
                                guard let at = line.atMs else { return }
                                library.request { try $0.seek(positionMs: at) }
                            }
                    }
                    Spacer(minLength: 160)
                }
                .padding(.horizontal, 24)
            }
            .onChange(of: active) { _, now in
                guard let now else { return }
                withAnimation(.smooth) { scroller.scrollTo(max(now - 2, 0), anchor: .top) }
            }
        }
        .safeAreaInset(edge: .bottom) {
            if let origin = sheet?.origin, !origin.isEmpty {
                Text("Lyrics: \(origin)").font(.caption2).foregroundStyle(.secondary).padding(8)
            }
        }
        .background(BackdropView(track: track))
        .navigationTitle(track?.shownTitle ?? "Lyrics")
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            if features.online, let track {
                Button("Refresh", systemImage: "arrow.clockwise") { features.loadLyrics(track.id, refresh: true) }
            }
        }
        .task(id: track?.id) {
            if let track { features.loadLyrics(track.id) }
        }
    }
}

struct LyricLine: View {
    let line: LyricRow
    let shining: Bool
    let timed: Bool
    let position: UInt64

    var body: some View {
        let color = shining && timed ? Color.accentColor : Color.primary
        Group {
            if line.words.isEmpty || !shining {
                Text(line.text)
            } else {
                Text(line.words.reduce(into: AttributedString()) { sung, word in
                    var part = AttributedString(word.text)
                    part.foregroundColor = position >= word.atMs ? color : color.opacity(0.4)
                    sung += part
                })
            }
        }
        .font(.title.weight(shining ? .bold : .medium))
        .foregroundStyle(color)
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}

struct SignalPlot: View {
    let values: [Float]
    var mirrored = false

    var body: some View {
        Canvas { context, size in
            guard values.count > 1 else { return }
            let peak = CGFloat(values.map { abs($0) }.max() ?? 1)
            let scale = peak > 0 ? 1 / peak : 1
            let step = size.width / CGFloat(values.count - 1)
            var path = Path()
            for (index, value) in values.enumerated() {
                let x = CGFloat(index) * step
                let level = CGFloat(abs(value)) * scale
                if mirrored {
                    path.move(to: CGPoint(x: x, y: size.height / 2 * (1 - level)))
                    path.addLine(to: CGPoint(x: x, y: size.height / 2 * (1 + level)))
                } else if index == 0 {
                    path.move(to: CGPoint(x: x, y: size.height * (1 - level)))
                } else {
                    path.addLine(to: CGPoint(x: x, y: size.height * (1 - level)))
                }
            }
            context.stroke(path, with: .color(.accentColor), lineWidth: mirrored ? max(step * 0.7, 1) : 1.5)
        }
        .accessibilityHidden(true)
    }
}

struct AnalysisView: View {
    @Environment(Library.self) private var library
    let chosen: Track?

    var body: some View {
        let features = library.features
        let track = chosen ?? library.snapshot?.current
        let report = features.analysis?.trackId == track?.id ? features.analysis : nil
        List {
            if let track {
                Section { CollectionRow(track: track, title: track.shownTitle, detail: track.quality) }
                Section {
                    if features.analysisBusy {
                        ProgressView(value: features.analysisProgress ?? 0) { Text("Analysing audio…") }
                        Button("Cancel analysis", role: .destructive) { features.cancelAnalysis() }
                    } else {
                        Button(report == nil ? "Analyse" : "Analyse again") { features.analyse(track.id) }
                    }
                }
            } else {
                Text("Choose a song to analyse").foregroundStyle(.secondary)
            }
            if let report {
                Section("Source audio") {
                    LabeledContent("Verdict", value: report.verdict)
                    ForEach(report.findings, id: \.self) { Text($0).font(.footnote) }
                }
                Section("Waveform") {
                    SignalPlot(values: report.waveform, mirrored: true).frame(height: 120)
                }
                Section("Spectrum") {
                    SignalPlot(values: report.spectrum).frame(height: 120)
                    if let last = report.spectrum.indices.last {
                        LabeledContent("Range", value: String(format: "0 – %.1f kHz", Double(report.spectrumStepHz) * Double(last) / 1000))
                    }
                }
                Section("Levels") {
                    LabeledContent("Loudness", value: report.loudnessLufs.map { String(format: "%.1f LUFS", $0) } ?? "Not measurable")
                    LabeledContent("Dynamic range", value: report.dynamicRange.map { "DR\($0)" } ?? "Not measurable")
                    LabeledContent("Peak", value: String(format: "%.2f dBFS", decibels(report.peak)))
                    LabeledContent("True peak", value: String(format: "%.2f dBTP", decibels(report.truePeak)))
                    LabeledContent("RMS", value: String(format: "%.2f dBFS", decibels(report.rms)))
                    LabeledContent("Clipped samples", value: "\(report.clipped)")
                    LabeledContent("Bits in use", value: report.bitsInUse.map { "\($0)" } ?? "Unknown")
                }
                if let track {
                    RecognitionSection(track: track)
                }
            }
        }
        .navigationTitle("Analysis")
        .navigationBarTitleDisplayMode(.inline)
    }

    private func decibels(_ linear: Float) -> Double {
        linear > 0 ? 20 * log10(Double(linear)) : -.infinity
    }
}

struct RecognitionSection: View {
    @Environment(Library.self) private var library
    let track: Track
    @State private var key = ""

    var body: some View {
        let features = library.features
        Section("Identify this recording") {
            if !features.online {
                Text("Enable online features in Settings to identify recordings.").foregroundStyle(.secondary)
            } else {
                SecureField("AcoustID key (optional)", text: $key)
                    .onAppear { key = features.recognitionKey }
                Button("Save recognition key") { features.saveRecognitionKey(key) }
                    .disabled(key == features.recognitionKey)
                Button(features.recognitionBusy ? "Identifying…" : "Identify by sound") { features.recognise(track.id) }
                    .disabled(features.recognitionBusy)
                ForEach(features.recognition, id: \.song.recording) { candidate in
                    VStack(alignment: .leading) {
                        Text(candidate.song.title).font(.body.weight(.medium))
                        Text("\(candidate.song.artist) · \(candidate.song.album)").font(.subheadline).foregroundStyle(.secondary)
                        Text("\(candidate.score)% match").font(.caption).foregroundStyle(.tint)
                    }
                }
            }
        }
    }
}

struct VisualizerView: View {
    @Environment(Library.self) private var library

    var body: some View {
        let samples = library.features.samples
        VStack(spacing: 20) {
            if library.snapshot?.transport != .playing, samples.isEmpty {
                Text("Play a song to see its sound").foregroundStyle(.secondary)
            }
            SignalPlot(values: samples, mirrored: true)
                .frame(maxWidth: .infinity, maxHeight: 320)
                .padding()
                .glassEffect(.regular, in: .rect(cornerRadius: 24))
            if let track = library.snapshot?.current {
                Text(track.shownTitle).font(.headline)
                Text(track.quality).font(.subheadline).foregroundStyle(.secondary)
            }
        }
        .padding()
        .background(BackdropView(track: library.snapshot?.current))
        .navigationTitle("Live waveform")
        .navigationBarTitleDisplayMode(.inline)
        .onAppear { library.features.visualize(true) }
        .onDisappear { library.features.visualize(false) }
    }
}

struct InspectorView: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    let track: Track

    var body: some View {
        let features = library.features
        let inspector = features.inspector?.track.id == track.id ? features.inspector : nil
        let shown = inspector?.track ?? track
        List {
            Section {
                CollectionRow(track: shown, title: shown.shownTitle, detail: shown.shownArtist)
            }
            Section("Tags") {
                LabeledContent("Album", value: shown.album.isEmpty ? "Not tagged" : shown.album)
                LabeledContent("Album artist", value: shown.albumArtist.isEmpty ? "Not tagged" : shown.albumArtist)
                LabeledContent("Genre", value: shown.genre.isEmpty ? "Not tagged" : shown.genre)
                LabeledContent("Date", value: shown.date.isEmpty ? "Not tagged" : shown.date)
                LabeledContent("Track", value: shown.trackNumber.map { "\($0)" } ?? "Not tagged")
                LabeledContent("Disc", value: shown.discNumber.map { "\($0)" } ?? "Not tagged")
                if let inspector {
                    if !inspector.isrc.isEmpty { LabeledContent("ISRC", value: inspector.isrc) }
                    if !inspector.copyright.isEmpty { LabeledContent("Copyright", value: inspector.copyright) }
                    if !inspector.comment.isEmpty { LabeledContent("Comment", value: inspector.comment) }
                }
                if let recording = shown.recording { LabeledContent("Recording", value: recording) }
                if let release = shown.release { LabeledContent("Release", value: release) }
            }
            Section("Audio") {
                LabeledContent("Codec", value: shown.codec)
                LabeledContent("Sample rate", value: "\(shown.rate) Hz")
                LabeledContent("Bit depth", value: shown.bits.map { "\($0)-bit" } ?? "Unknown")
                LabeledContent("Channels", value: "\(shown.channels)")
                LabeledContent("Duration", value: clock(shown.durationMs))
                if let inspector {
                    LabeledContent("Container", value: inspector.container)
                    LabeledContent("Sample format", value: inspector.sampleFormat)
                    LabeledContent("Seekable", value: inspector.seekable ? "Yes" : "No")
                    if !inspector.encoder.isEmpty { LabeledContent("Encoder", value: inspector.encoder) }
                }
                LabeledContent("Document", value: shown.name)
            }
            Section {
                Button("Lyrics", systemImage: "quote.bubble") { navigation.open(.lyrics(shown)) }
                Button("Analyse", systemImage: "waveform.path.ecg") { navigation.open(.analysis(shown)) }
                if features.online {
                    Button("Enrich metadata, artwork and lyrics", systemImage: "sparkles") { features.enrich([shown]) }
                        .disabled(features.enriching)
                    Button("Artist details", systemImage: "music.mic") { features.loadArtist(shown.id) }
                }
            }
            if let artist = features.artist {
                ArtistInformationSection(artist: artist)
            }
        }
        .navigationTitle("Song details")
        .navigationBarTitleDisplayMode(.inline)
        .onAppear { features.loadInspector(track.id) }
    }
}

struct ArtistInformationSection: View {
    let artist: ArtistInformation

    var body: some View {
        Section(artist.name) {
            if let picture = artist.picture, let image = UIImage(data: picture) {
                Image(uiImage: image)
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
    }
}
