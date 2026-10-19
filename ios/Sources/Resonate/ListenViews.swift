import ResonateNative
import SwiftUI
import UIKit

struct ListenSheet: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    @Environment(\.dismiss) private var dismiss
    @Environment(\.scenePhase) private var scenePhase
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var token = ""

    var body: some View {
        let listening = library.features.listening
        let shown: HeardSong? = if case .found(let song) = listening.stage { song } else { nil }
        let earlier = listening.heard.filter { $0.key != shown?.key }

        NavigationStack {
            List {
                Section {
                    ListenStageView(listening: listening, find: find, openSettings: openSettings)
                }
                Section {
                    Toggle(
                        "Keep listening",
                        isOn: Binding(get: { listening.keepsListening }, set: { listening.keepListening($0) })
                    )
                    if let note = followingNote(listening.following) {
                        Label(note, systemImage: "waveform")
                            .font(.subheadline)
                            .foregroundStyle(.secondary)
                            .symbolEffect(.variableColor.iterative, isActive: listening.following == .listening && !reduceMotion)
                    }
                } footer: {
                    Text("Listens for the next song every \(Listening.followedEverySeconds) seconds while this stays open. Resonate pauses while it listens, so the microphone hears the room.")
                }
                if !earlier.isEmpty {
                    Section("Heard earlier") {
                        ForEach(earlier, id: \.key) { song in
                            Button { find(song) } label: { HeardRow(song: song) }
                                .buttonStyle(.plain)
                        }
                    }
                }
                Section {
                    SecureField("AudD token (optional)", text: $token)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                    Button("Save token") { listening.saveAuddToken(token) }
                        .disabled(token == listening.auddToken)
                } header: {
                    Text("Recognition")
                } footer: {
                    Text("Listening sends Shazam a signature of what was heard, never the sound itself. With an AudD token, AudD is sent the clip when Shazam does not know the song.")
                }
            }
            .navigationTitle("Listen")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button(role: .close) { dismiss() }
                }
            }
            .sensoryFeedback(.success, trigger: listening.named)
        }
        .onAppear {
            token = listening.auddToken
            listening.listen()
        }
        .onChange(of: listening.auddToken) { kept, now in
            if token == kept { token = now }
        }
        .onDisappear { listening.stop() }
        .onChange(of: scenePhase) { _, phase in
            if phase == .background { listening.stop() }
        }
    }

    private func followingNote(_ following: ListenFollowing) -> String? {
        switch following {
        case .off: nil
        case .waiting: "Listening for the next song every \(Listening.followedEverySeconds) seconds"
        case .listening: "Listening for the next song…"
        }
    }

    private func find(_ song: HeardSong) {
        dismiss()
        navigation.nowPlaying = false
        navigation.tab = .search
        library.tabSearch.text = [song.artist, song.title].filter { !$0.isEmpty }.joined(separator: " ")
    }

    private func openSettings() {
        dismiss()
        navigation.tab = .settings
    }
}

struct ListenStageView: View {
    @Environment(\.openURL) private var openURL
    @ScaledMetric private var control: CGFloat = 88

    let listening: Listening
    let find: (HeardSong) -> Void
    let openSettings: () -> Void

    var body: some View {
        let stage = listening.stage
        VStack(spacing: 16) {
            if case .found(let song) = stage {
                HeardCard(song: song)
            } else {
                ListenControl(listening: listening, edge: control)
            }
            if let note = note(stage) {
                Text(note)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
                    .contentTransition(.opacity)
            }
            switch stage {
            case .recording(_, let since):
                ProgressView(
                    timerInterval: since...since.addingTimeInterval(Listening.clipSeconds),
                    countsDown: false,
                    label: { EmptyView() },
                    currentValueLabel: { EmptyView() }
                )
            case .asking:
                ProgressView()
            case .found(let song):
                actions(song)
            case .offline:
                Button("Open Settings", action: openSettings).buttonStyle(.glass)
            case .denied:
                Button("Allow in Settings") {
                    if let settings = URL(string: UIApplication.openSettingsURLString) { openURL(settings) }
                }
                .buttonStyle(.glass)
            case .idle, .unknown, .silent, .unreached, .failed:
                EmptyView()
            }
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 12)
        .motion(.smooth, value: stage)
        .onChange(of: stage) { _, now in
            if let spoken = spoken(now) { AccessibilityNotification.Announcement(spoken).post() }
        }
    }

    private func spoken(_ stage: ListenStage) -> String? {
        switch stage {
        case .found(let song): "Heard \(song.title)" + (song.artist.isEmpty ? "" : ", by \(song.artist)")
        case .unknown, .silent, .unreached, .offline, .denied, .failed: note(stage)
        case .idle, .recording, .asking: nil
        }
    }

    private func actions(_ song: HeardSong) -> some View {
        HStack(spacing: 12) {
            Button { find(song) } label: { Label("Find it", systemImage: "magnifyingglass") }
                .buttonStyle(.glassProminent)
            if let link = URL(string: song.link), !song.link.isEmpty {
                Button { openURL(link) } label: { Label("Open", systemImage: "arrow.up.forward.app") }
                    .buttonStyle(.glass)
            }
            Button { listening.listen() } label: { Label("Listen again", systemImage: "ear") }
                .buttonStyle(.glass)
        }
        .labelStyle(.titleAndIcon)
        .font(.subheadline.weight(.medium))
    }

    private func note(_ stage: ListenStage) -> String? {
        switch stage {
        case .idle: "Hear what is sounding and name it."
        case .recording(1, _): "Listening for \(Int(Listening.clipSeconds)) seconds…"
        case .recording(let nth, _): "Nothing named yet, so listening again (\(nth) of \(Listening.triesOnAMiss))…"
        case .asking: "Asking who it is…"
        case .found: nil
        case .unknown: "Nothing that was heard is a song the services know."
        case .silent: "Nothing reached the microphone. Is anything playing?"
        case .unreached(let service): "\(service) could not be reached. Is this iPhone online?"
        case .offline: "Online features are off in Settings, so nothing can name what is heard."
        case .denied: "Resonate may not use the microphone. Allow it in the Settings app to listen."
        case .failed(let reason): reason
        }
    }
}

struct ListenControl: View {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    let listening: Listening
    let edge: CGFloat

    var body: some View {
        let recording = if case .recording = listening.stage { true } else { false }
        let busy = listening.stage.isListening
        Button {
            if busy { listening.stop() } else { listening.listen() }
        } label: {
            Image(systemName: busy ? "waveform" : "ear")
                .font(.system(size: edge * 0.38, weight: .medium))
                .symbolEffect(.variableColor.iterative, isActive: recording && !reduceMotion)
                .foregroundStyle(busy ? Color.wornInk : Color.worn)
                .frame(width: edge, height: edge)
                .contentShape(.circle)
        }
        .buttonStyle(.plain)
        .glassEffect(busy ? .regular.tint(Color.worn).interactive() : .regular.interactive(), in: .circle)
        .accessibilityLabel(busy ? "Stop listening" : "Listen")
        .accessibilityHint(busy ? "" : "Records a few seconds from the microphone and names the song")
    }
}

struct HeardCard: View {
    @ScaledMetric private var edge: CGFloat = 168

    let song: HeardSong
    @State private var picture: UIImage?

    var body: some View {
        VStack(spacing: 12) {
            Group {
                if let picture {
                    Image(uiImage: picture).resizable().scaledToFill()
                } else {
                    Rectangle().fill(.quaternary)
                        .overlay { Image(systemName: "music.note").font(.largeTitle).foregroundStyle(.secondary) }
                }
            }
            .frame(width: edge, height: edge)
            .clipShape(.rect(cornerRadius: 12, style: .continuous))
            .shadow(color: .black.opacity(0.18), radius: 12, y: 6)
            .accessibilityHidden(true)

            VStack(spacing: 4) {
                Text(song.title)
                    .font(.title3.weight(.semibold))
                if !song.artist.isEmpty {
                    Text(song.artist).font(.body).foregroundStyle(.tint)
                }
                if let detail = detail {
                    Text(detail).font(.subheadline).foregroundStyle(.secondary)
                }
                Text("Named by \(song.by)")
                    .font(.caption)
                    .foregroundStyle(.tertiary)
            }
            .multilineTextAlignment(.center)
            .accessibilityElement(children: .combine)
        }
        .task(id: song.key) {
            guard let data = song.picture else {
                picture = nil
                return
            }
            picture = try? await Work.aside {
                guard let image = UIImage(data: data) else { return nil }
                return image.preparingForDisplay() ?? image
            }
        }
    }

    private var detail: String? {
        let parts = [song.album, song.year.map(String.init) ?? ""].filter { !$0.isEmpty }
        return parts.isEmpty ? nil : parts.joined(separator: " · ")
    }
}

struct HeardRow: View {
    let song: HeardSong

    var body: some View {
        HStack {
            VStack(alignment: .leading, spacing: 2) {
                Text(song.title).lineLimit(1)
                Text([song.artist, song.album].filter { !$0.isEmpty }.joined(separator: " · "))
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
            Spacer(minLength: 8)
            Image(systemName: "magnifyingglass").foregroundStyle(.tertiary)
        }
        .contentShape(.rect)
        .accessibilityHint("Searches for this song")
    }
}
