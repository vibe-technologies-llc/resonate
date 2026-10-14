import AVKit
import ResonateNative
import SwiftUI

struct MiniPlayer: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    @Environment(\.tabViewBottomAccessoryPlacement) private var placement

    var body: some View {
        let state = library.snapshot
        let track = state?.current
        HStack(spacing: 12) {
            ArtworkView(track: track, corner: 8)
                .frame(width: 36, height: 36)
            VStack(alignment: .leading, spacing: 0) {
                Text(track?.shownTitle ?? "").font(.subheadline.weight(.semibold)).lineLimit(1)
                if placement != .inline {
                    Text(track?.shownArtist ?? "").font(.caption).foregroundStyle(.secondary).lineLimit(1)
                }
            }
            .contentTransition(.opacity)
            .animation(.smooth, value: track?.id)
            Spacer(minLength: 0)
            if let track, placement != .inline {
                FavouriteButton(track: track)
            }
            PlayButton(playing: state?.transport == .playing)
            if placement != .inline {
                Button("Next", systemImage: "forward.fill") { library.request { try $0.next() } }
                    .labelStyle(.iconOnly)
            }
        }
        .padding(.horizontal, 14)
        .contentShape(.rect)
        .onTapGesture { navigation.nowPlaying = true }
        .accessibilityAction(named: "Open now playing") { navigation.nowPlaying = true }
    }
}

struct PlayButton: View {
    @Environment(Library.self) private var library
    let playing: Bool
    var large = false

    var body: some View {
        Button {
            library.toggle()
        } label: {
            Image(systemName: playing ? "pause.fill" : "play.fill")
                .font(large ? .system(size: 34, weight: .semibold) : .title3)
                .frame(width: large ? 76 : 32, height: large ? 76 : 32)
                .contentTransition(.symbolEffect(.replace))
        }
        .buttonStyle(.plain)
        .modifier(GlassWhen(enabled: large))
        .accessibilityLabel(playing ? "Pause" : "Play")
    }
}

struct GlassWhen: ViewModifier {
    let enabled: Bool

    func body(content: Content) -> some View {
        if enabled {
            content.glassEffect(.regular.interactive(), in: .circle)
        } else {
            content
        }
    }
}

struct NowPlayingView: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    @Environment(\.dismiss) private var dismiss
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var scrubbing: Double?
    @State private var choosingPlaylist = false

    var body: some View {
        let state = library.snapshot
        let track = state?.current
        let playing = state?.transport == .playing
        let duration = Double(max(state?.durationMs ?? 0, 1))
        NavigationStack {
            VStack(spacing: 22) {
                Spacer(minLength: 0)
                ArtworkView(track: track, corner: 24)
                    .aspectRatio(1, contentMode: .fit)
                    .shadow(color: .black.opacity(0.3), radius: 28, y: 16)
                    .scaleEffect(playing || reduceMotion ? 1 : 0.86)
                    .animation(.spring(duration: 0.5, bounce: 0.25), value: playing)
                    .padding(.horizontal, 12)
                HStack(alignment: .center) {
                    VStack(alignment: .leading, spacing: 4) {
                        Text(track?.shownTitle ?? "Nothing playing").font(.title2.weight(.bold)).lineLimit(2)
                        if let track {
                            Button(track.shownArtist) {
                                dismiss()
                                navigation.open(.artist(track.artist))
                            }
                            .font(.title3)
                            .foregroundStyle(.secondary)
                            .accessibilityLabel("Open artist \(track.artist)")
                            if !track.album.isEmpty {
                                Button(track.album) {
                                    dismiss()
                                    navigation.open(.album(title: track.album, artist: track.groupingArtist))
                                }
                                .font(.subheadline)
                                .foregroundStyle(.tertiary)
                                .accessibilityLabel("Open album \(track.album)")
                            }
                        }
                    }
                    .buttonStyle(.plain)
                    .lineLimit(1)
                    Spacer()
                    if let track { FavouriteButton(track: track).font(.title2) }
                }
                VStack(spacing: 4) {
                    Slider(
                        value: Binding(
                            get: { scrubbing ?? Double(state?.positionMs ?? 0) },
                            set: { scrubbing = $0 }
                        ),
                        in: 0...duration
                    ) { editing in
                        if !editing, let target = scrubbing {
                            let position = UInt64(target)
                            library.request { try $0.seek(positionMs: position) }
                            scrubbing = nil
                        }
                    }
                    .accessibilityLabel("Song progress")
                    HStack {
                        Text(clock(UInt64(scrubbing ?? Double(state?.positionMs ?? 0))))
                        Spacer()
                        if let track { Text(track.quality) }
                        Spacer()
                        Text(clock(state?.durationMs ?? 0))
                    }
                    .font(.caption.monospacedDigit())
                    .foregroundStyle(.secondary)
                }
                GlassEffectContainer(spacing: 24) {
                    HStack(spacing: 28) {
                        Button("Shuffle", systemImage: "shuffle") {
                            let shuffled = !(state?.shuffle ?? false)
                            library.request { try $0.setShuffle(shuffled: shuffled) }
                        }
                        .foregroundStyle(state?.shuffle == true ? Color.accentColor : .secondary)
                        Button("Previous", systemImage: "backward.fill") { library.request { try $0.previous() } }
                            .font(.title)
                        PlayButton(playing: playing, large: true)
                        Button("Next", systemImage: "forward.fill") { library.request { try $0.next() } }
                            .font(.title)
                        Button("Repeat", systemImage: state?.repeat == 1 ? "repeat.1" : "repeat") {
                            let next = ((state?.repeat ?? 0) + 1) % 3
                            library.request { try $0.setRepeat(repeat: next) }
                        }
                        .foregroundStyle((state?.repeat ?? 0) > 0 ? Color.accentColor : .secondary)
                    }
                    .labelStyle(.iconOnly)
                    .buttonStyle(.plain)
                }
                HStack {
                    Image(systemName: "speaker.fill").foregroundStyle(.secondary)
                    Slider(
                        value: Binding(
                            get: { Double(state?.volume ?? 1) },
                            set: { value in library.request { try $0.setVolume(volume: Float(value)) } }
                        ),
                        in: 0...1
                    )
                    .accessibilityLabel("Player volume")
                    Image(systemName: "speaker.wave.3.fill").foregroundStyle(.secondary)
                }
                HStack {
                    tool("Lyrics", "quote.bubble") { navigation.open(.lyrics(nil)) }
                    tool("Equaliser", "slider.vertical.3") {
                        navigation.tab = .settings
                        navigation.open(.sound)
                    }
                    tool("Sleep timer", state?.sleep == nil ? "moon" : "moon.fill") { navigation.sleepTimer = true }
                    tool("Visualizer", "waveform") { navigation.open(.visualizer) }
                    RoutePicker().frame(width: 44, height: 44)
                }
                .labelStyle(.iconOnly)
                .font(.title3)
                Spacer(minLength: 0)
            }
            .padding(.horizontal, 28)
            .background(BackdropView(track: track))
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button("Close", systemImage: "chevron.down") { dismiss() }
                }
                ToolbarItem(placement: .principal) {
                    Text("Now playing").font(.headline)
                }
                ToolbarItem(placement: .topBarTrailing) {
                    Menu("More", systemImage: "ellipsis") {
                        if let track {
                            Button("Add to playlist", systemImage: "text.badge.plus") { choosingPlaylist = true }
                            Button("Song details", systemImage: "info.circle") { open(.inspector(track)) }
                            Button("Analyse", systemImage: "waveform.path.ecg") { open(.analysis(track)) }
                        }
                        Button("Output", systemImage: "hifispeaker") {
                            navigation.tab = .settings
                            open(.output)
                        }
                    }
                }
            }
            .sheet(isPresented: $choosingPlaylist) {
                if let track { PlaylistPicker(tracks: [track]) }
            }
        }
    }

    private func open(_ destination: Destination) {
        dismiss()
        navigation.open(destination)
    }

    private func tool(_ title: String, _ symbol: String, _ action: @escaping () -> Void) -> some View {
        Button(title, systemImage: symbol) {
            dismiss()
            action()
        }
        .frame(maxWidth: .infinity)
        .buttonStyle(.plain)
        .foregroundStyle(.secondary)
    }
}

struct RoutePicker: UIViewRepresentable {
    func makeUIView(context: Context) -> AVRoutePickerView {
        let picker = AVRoutePickerView()
        picker.prioritizesVideoDevices = false
        return picker
    }

    func updateUIView(_ uiView: AVRoutePickerView, context: Context) {}
}

struct QueueView: View {
    @Environment(Library.self) private var library

    var body: some View {
        List {
            HandoffRows()
            if library.queue.isEmpty {
                Text("Your queue is empty").foregroundStyle(.secondary)
            }
            ForEach(library.queue, id: \.queueId) { track in
                TrackRow(track: track, current: library.snapshot?.current?.queueId == track.queueId) {
                    library.jump(to: track)
                }
            }
        }
        .listStyle(.plain)
        .navigationTitle("Queue")
        .toolbar { LibraryToolbar() }
    }
}

struct HandoffRows: View {
    @Environment(Library.self) private var library

    var body: some View {
        ForEach(library.features.syncHandoffs, id: \.device) { handoff in
            Button {
                library.features.continueFrom(handoff)
            } label: {
                Label(
                    "Continue \(counted(Int(handoff.songs), "song")) from \(handoff.device)\(handoff.playing ? " · playing" : "")",
                    systemImage: "arrow.triangle.2.circlepath"
                )
            }
        }
    }
}

struct SleepTimerSheet: View {
    @Environment(Library.self) private var library
    @Environment(\.dismiss) private var dismiss
    private static let minutes: [UInt32] = [5, 15, 30, 45, 60, 90]

    var body: some View {
        let state = library.snapshot
        NavigationStack {
            List {
                Section {
                    if let remaining = state?.sleepRemainingMs {
                        Text("Pausing in \(clock(remaining))").font(.headline)
                    } else if state?.sleep == .endOfTrack {
                        Text("Pauses at the end of this song")
                    } else if state?.sleep == .endOfQueue {
                        Text("Pauses at the end of the queue")
                    } else {
                        Text("No timer set").foregroundStyle(.secondary)
                    }
                }
                Section {
                    ForEach(Self.minutes, id: \.self) { minutes in
                        Button("\(minutes) minutes") { set(.after(seconds: minutes * 60)) }
                    }
                    Button("End of song") { set(.endOfTrack) }
                    Button("End of queue") { set(.endOfQueue) }
                }
                if state?.sleep != nil {
                    Button("Cancel timer", role: .destructive) { set(nil) }
                }
            }
            .navigationTitle("Sleep timer")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar { Button("Done") { dismiss() } }
        }
    }

    private func set(_ timer: SleepTimer?) {
        library.request { try $0.setSleep(timer: timer) }
        dismiss()
    }
}
