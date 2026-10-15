import AVKit
import MediaPlayer
import ResonateNative
import SwiftUI

struct MiniPlayer: View {
    private static let swipeDistance: CGFloat = 40
    private static let titledFrom: CGFloat = 150
    private static let skipsFrom: CGFloat = 200
    private static let inlineInset: CGFloat = 12
    private static let artistOpacity = 0.6

    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    @Environment(\.tabViewBottomAccessoryPlacement) private var placement
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @ScaledMetric(relativeTo: .subheadline) private var artworkEdge: CGFloat = 36
    @ScaledMetric(relativeTo: .subheadline) private var inlineArtworkEdge: CGFloat = 28
    let zoom: Namespace.ID
    @State private var skipped = 0
    @State private var width: CGFloat = 0

    var body: some View {
        let track = library.snapshot?.current
        let expanded = placement != .inline
        let titled = expanded || width == 0 || width >= Self.titledFrom
        let skips = expanded || width >= Self.skipsFrom
        HStack(spacing: expanded ? 12 : 8) {
            ArtworkView(track: track, corner: expanded ? 8 : 7)
                .frame(width: expanded ? artworkEdge : inlineArtworkEdge, height: expanded ? artworkEdge : inlineArtworkEdge)
                .matchedTransitionSource(id: PlayerTransition.source, in: zoom)
            if titled {
                VStack(alignment: .leading, spacing: 0) {
                    Text(track?.shownTitle ?? "")
                        .font(expanded ? .subheadline.weight(.semibold) : .footnote.weight(.semibold))
                        .lineLimit(1)
                    if expanded {
                        Text(track?.shownArtist ?? "")
                            .font(.caption)
                            .foregroundStyle(Color.primary.opacity(Self.artistOpacity))
                            .lineLimit(1)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .contentTransition(reduceMotion ? .identity : .opacity)
                .motion(.smooth, value: track?.id)
            } else {
                Spacer(minLength: 0)
            }
            if let track, expanded {
                FavouriteButton(track: track)
            }
            if expanded {
                Button("Previous", systemImage: "backward.fill") { skip(forward: false) }
                    .labelStyle(.iconOnly)
                    .touchTarget()
            }
            PlayButton(transport: library.transport)
            if skips {
                Button("Next", systemImage: "forward.fill") { skip(forward: true) }
                    .labelStyle(.iconOnly)
                    .touchTarget()
            }
        }
        .buttonStyle(.plain)
        .padding(.horizontal, expanded ? 14 : Self.inlineInset)
        .frame(maxWidth: .infinity)
        .onGeometryChange(for: CGFloat.self) { $0.size.width } action: { width = $0 }
        .contentShape(.rect)
        .onTapGesture { navigation.nowPlaying = true }
        .simultaneousGesture(
            DragGesture(minimumDistance: 24).onEnded { drag in
                guard abs(drag.translation.width) > max(abs(drag.translation.height), Self.swipeDistance) else { return }
                skip(forward: drag.translation.width < 0)
            }
        )
        .sensoryFeedback(.impact(weight: .light), trigger: skipped)
        .accessibilityElement(children: .contain)
        .accessibilityLabel(track.map { "\($0.shownTitle), \($0.shownArtist)" } ?? "")
        .accessibilityAction(named: "Open now playing") { navigation.nowPlaying = true }
        .accessibilityAction(named: "Previous") { skip(forward: false) }
        .accessibilityAction(named: "Next") { skip(forward: true) }
    }

    private func skip(forward: Bool) {
        skipped += 1
        if forward { library.next() } else { library.previous() }
    }
}

extension PlayClock {
    var playedFraction: Double {
        durationMs > 0 ? min(Double(positionMs) / Double(durationMs), 1) : 0
    }
}

struct PlayButton: View {
    @Environment(Library.self) private var library
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    let transport: Transport
    var large = false
    @State private var pressed = 0

    var body: some View {
        let playingish = transport == .playing || transport == .loading
        Button {
            pressed += 1
            library.toggle()
        } label: {
            Image(systemName: playingish ? "pause.fill" : "play.fill")
                .font(large ? .system(size: 34, weight: .semibold) : .title3)
                .frame(width: large ? 76 : 32, height: large ? 76 : 32)
                .contentTransition(reduceMotion ? .identity : .symbolEffect(.replace))
        }
        .buttonStyle(.plain)
        .modifier(GlassWhen(enabled: large))
        .sensoryFeedback(.impact(weight: .medium), trigger: pressed)
        .accessibilityLabel(playingish ? "Pause" : "Play")
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

enum PlayerMode {
    case artwork
    case lyrics
    case queue
}

struct NowPlayingView: View {
    private static let gutter: CGFloat = 24
    private static let reshaping: Animation = .spring(duration: 0.45, bounce: 0.15)
    private static let arriving: Animation = .easeOut(duration: 0.2)
    private static let leaving: Animation = .easeOut(duration: 0.12)

    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    @Environment(\.dismiss) private var dismiss
    @Environment(\.dynamicTypeSize) private var typeSize
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var mode = PlayerMode.artwork
    @State private var settled: PlayerMode?
    @State private var picked: PickedTracks?

    var body: some View {
        let track = library.snapshot?.current
        NavigationStack {
            Group {
                if mode == .artwork, typeSize.isAccessibilitySize {
                    ScrollView { layout(track) }
                } else {
                    layout(track)
                }
            }
            .overlayPreferenceValue(CoverSlot.self) { slots in
                GeometryReader { proxy in
                    let place = CoverPlace(mode)
                    if let slot = slots[place] {
                        MovingCover(track: track, slot: proxy[slot], large: place == .large)
                    }
                }
                .allowsHitTesting(false)
            }
            .padding(.horizontal, Self.gutter)
            .background(BackdropView(track: track))
            .safeAreaInset(edge: .top) { StatusBanner() }
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button("Close", systemImage: "chevron.down") { dismiss() }
                }
                ToolbarItem(placement: .topBarTrailing) {
                    PlayerMoreMenu(track: track) { picked = PickedTracks(tracks: [$0]) }
                }
            }
            .sheet(item: $picked) { PlaylistPicker(tracks: $0.tracks) }
        }
    }

    private func layout(_ track: Track?) -> some View {
        let large = mode == .artwork
        return VStack(spacing: large ? 20 : 16) {
            if large {
                Spacer(minLength: 0)
                LargeArtwork()
                PlayerTitles(track: track)
            } else {
                CompactTitles(track: track)
                    .padding(.top, 8)
                ZStack {
                    if settled == mode {
                        focusedContent(track).transition(.opacity)
                    }
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
            PlayerScrubber()
            TransportRow()
            if large {
                SleepCountdown()
                VolumeRow()
            }
            PlayerModes(mode: mode, choose: choose)
            if large {
                Spacer(minLength: 0)
            }
        }
        .padding(.bottom, large ? 0 : 8)
    }

    @ViewBuilder
    private func focusedContent(_ track: Track?) -> some View {
        switch mode {
        case .lyrics: LyricsPane(track: track, compact: true)
        case .queue, .artwork: UpNextList().padding(.horizontal, -Self.gutter)
        }
    }

    private func choose(_ target: PlayerMode) {
        guard !reduceMotion else {
            mode = target
            settled = target == .artwork ? nil : target
            return
        }
        let coverMoves = (mode == .artwork) != (target == .artwork)
        guard coverMoves else {
            withAnimation(Self.arriving) {
                mode = target
                settled = target
            }
            return
        }
        withAnimation(Self.leaving) { settled = nil }
        withAnimation(Self.reshaping, completionCriteria: .logicallyComplete) {
            mode = target
        } completion: {
            guard mode == target, target != .artwork else { return }
            withAnimation(Self.arriving) { settled = target }
        }
    }
}

enum CoverPlace {
    case large
    case compact

    init(_ mode: PlayerMode) {
        self = mode == .artwork ? .large : .compact
    }
}

struct CoverSlot: PreferenceKey {
    static var defaultValue: [CoverPlace: Anchor<CGRect>] { [:] }

    static func reduce(value: inout [CoverPlace: Anchor<CGRect>], nextValue: () -> [CoverPlace: Anchor<CGRect>]) {
        value.merge(nextValue()) { $1 }
    }
}

struct LargeArtwork: View {
    var body: some View {
        Color.clear
            .aspectRatio(1, contentMode: .fit)
            .anchorPreference(key: CoverSlot.self, value: .bounds) { [.large: $0] }
            .padding(.horizontal, 8)
    }
}

struct MovingCover: View {
    private static let pausedScale: CGFloat = 0.84

    @Environment(Library.self) private var library
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    let track: Track?
    let slot: CGRect
    let large: Bool
    @State private var edge: CGFloat = 0

    var body: some View {
        let playing = library.transport == .playing || library.transport == .loading
        let drawn = edge > 0 ? edge : max(slot.width, 1)
        let resting = large && !playing && !reduceMotion ? Self.pausedScale : 1
        ArtworkView(track: track, corner: 24)
            .frame(width: drawn, height: drawn)
            .shadow(color: .black.opacity(0.3), radius: 28, y: 16)
            .scaleEffect(resting)
            .motion(.spring(duration: 0.5, bounce: 0.25), value: playing)
            .scaleEffect(slot.width / drawn)
            .offset(x: slot.midX - drawn / 2, y: slot.midY - drawn / 2)
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            .onChange(of: slot, initial: true) { _, now in
                if large, now.width > 0, now.width != edge { edge = now.width }
            }
    }
}

struct PlayerTitles: View {
    let track: Track?

    var body: some View {
        HStack(alignment: .center, spacing: 12) {
            VStack(alignment: .leading, spacing: 4) {
                if let track {
                    SongMenu(track: track) {
                        Text(track.shownTitle)
                            .font(.title2.weight(.bold))
                            .foregroundStyle(Color.primary)
                            .lineLimit(2)
                            .multilineTextAlignment(.leading)
                    }
                    SongMenu(track: track) {
                        Text(track.shownArtist)
                            .font(.title3)
                            .foregroundStyle(Color.secondary)
                    }
                    if !track.album.isEmpty {
                        SongMenu(track: track) {
                            Text(track.album)
                                .font(.subheadline)
                                .foregroundStyle(Color.secondary.opacity(0.8))
                        }
                    }
                } else {
                    Text("Nothing playing")
                        .font(.title2.weight(.bold))
                        .lineLimit(2)
                }
            }
            .lineLimit(1)
            Spacer(minLength: 0)
            if let track { FavouriteButton(track: track).font(.title2) }
        }
    }
}

struct SongMenu<Label: View>: View {
    let track: Track
    @ViewBuilder let label: () -> Label

    var body: some View {
        Menu {
            SongDestinations(track: track)
        } label: {
            label()
        }
        .menuStyle(.button)
        .buttonStyle(.plain)
        .tint(Color.primary)
        .accessibilityHint("Shows the album, artist, song details and a link to share")
    }
}

struct SongDestinations: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    let track: Track

    var body: some View {
        if let albumPage = track.albumPage {
            Button("Go to album", systemImage: "square.stack") { navigation.open(albumPage) }
        }
        if !track.artist.isEmpty {
            Button("Go to artist", systemImage: "music.mic") { navigation.open(.artist(track.artist)) }
        }
        Button("Song details", systemImage: "info.circle") {
            navigation.open(.inspector(track))
        }
        Button("Share link", systemImage: "square.and.arrow.up") { library.features.share(track) }
    }
}

struct CompactTitles: View {
    @ScaledMetric(relativeTo: .headline) private var artworkEdge: CGFloat = 56
    let track: Track?

    var body: some View {
        HStack(spacing: 12) {
            Color.clear
                .frame(width: artworkEdge, height: artworkEdge)
                .anchorPreference(key: CoverSlot.self, value: .bounds) { [.compact: $0] }
            if let track {
                SongMenu(track: track) {
                    VStack(alignment: .leading, spacing: 2) {
                        Text(track.shownTitle).font(.headline).foregroundStyle(Color.primary).lineLimit(1)
                        Text(track.shownArtist).font(.subheadline).foregroundStyle(Color.secondary).lineLimit(1)
                    }
                    .accessibilityElement(children: .combine)
                }
            } else {
                Text("Nothing playing").font(.headline).lineLimit(1)
            }
            Spacer(minLength: 0)
            if let track { FavouriteButton(track: track).font(.title3) }
        }
    }
}

struct PlayerScrubber: View {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    private static let nearEnoughMs: Double = 1500
    private static let holdAtMost: Duration = .seconds(1)
    private static let restingHeight: CGFloat = 7
    private static let grabbedHeight: CGFloat = 14
    private static let touchHeight: CGFloat = 28
    private static let stepMs: Double = 10_000
    private static let slightestMove: CGFloat = 2

    @Environment(Library.self) private var library
    @State private var dragging: Double?
    @State private var grabbedAt: Double?
    @State private var held: Double?
    @State private var grabs = 0

    var body: some View {
        let timing = library.clock
        let duration = Double(max(timing.durationMs, 1))
        let live = Double(timing.positionMs)
        let shown = min(dragging ?? held ?? live, duration)
        let grabbed = dragging != nil

        VStack(spacing: 6) {
            GeometryReader { space in
                let width = max(space.size.width, 1)
                Capsule()
                    .fill(.primary.opacity(0.16))
                    .overlay(alignment: .leading) {
                        Rectangle()
                            .fill(.primary.opacity(grabbed ? 0.95 : 0.7))
                            .frame(width: width * shown / duration)
                    }
                    .clipShape(Capsule())
                    .frame(height: grabbed ? Self.grabbedHeight : Self.restingHeight)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .contentShape(Rectangle())
                    .gesture(drag(across: width, of: duration, from: shown))
            }
            .frame(height: Self.touchHeight)
            .motion(.spring(duration: 0.3, bounce: 0.3), value: grabbed)
            HStack {
                Text(clock(UInt64(shown)))
                Spacer()
                QualityLine(track: library.snapshot?.current)
                Spacer()
                Text("-" + clock(UInt64(max(duration - shown, 0))))
            }
            .font(.caption.monospacedDigit())
            .foregroundStyle(grabbed ? .primary : .secondary)
            .motion(.smooth, value: grabbed)
        }
        .allowsHitTesting(library.loaded)
        .sensoryFeedback(.impact(weight: .light), trigger: grabs)
        .accessibilityElement()
        .accessibilityLabel("Song position")
        .accessibilityValue("\(clock(UInt64(shown))) of \(clock(timing.durationMs))")
        .accessibilityAdjustableAction { direction in
            let step = direction == .increment ? Self.stepMs : -Self.stepMs
            commit(min(max(shown + step, 0), duration))
        }
        .onChange(of: timing.positionMs) { _, now in
            if let target = held, abs(Double(now) - target) < Self.nearEnoughMs { held = nil }
        }
        .task(id: held) {
            guard held != nil else { return }
            try? await Task.sleep(for: Self.holdAtMost)
            if !Task.isCancelled { held = nil }
        }
    }

    private func drag(across width: CGFloat, of duration: Double, from shown: Double) -> some Gesture {
        DragGesture(minimumDistance: 0)
            .onChanged { change in
                let start = grabbedAt ?? shown
                if grabbedAt == nil {
                    grabbedAt = start
                    grabs += 1
                }
                let moved = Double(change.translation.width / width) * duration
                dragging = min(max(start + moved, 0), duration)
            }
            .onEnded { change in
                grabbedAt = nil
                let target = dragging
                dragging = nil
                guard let target, abs(change.translation.width) >= Self.slightestMove else { return }
                commit(target)
            }
    }

    private func commit(_ target: Double) {
        held = target
        library.seek(to: UInt64(target))
    }
}

struct QualityLine: View {
    @Environment(Library.self) private var library
    let track: Track?

    var body: some View {
        let output = library.clock.outputRate
        let line = Self.line(track, output: output)
        if !line.trimmingCharacters(in: .whitespaces).isEmpty {
            Text(line)
                .font(.caption2.weight(.semibold))
                .foregroundStyle(.secondary)
                .lineLimit(1)
                .minimumScaleFactor(0.7)
                .padding(.horizontal, 7)
                .padding(.vertical, 2)
                .background(.quaternary, in: .capsule)
        }
    }

    private static func line(_ track: Track?, output: UInt32) -> String {
        guard let track else { return " " }
        var parts = [track.codec]
        if let bits = track.bits { parts.append("\(bits)-bit") }
        if track.rate > 0 { parts.append(kilohertz(track.rate)) }
        if output > 0 { parts.append("Output \(kilohertz(output))") }
        return parts.filter { !$0.isEmpty }.joined(separator: " · ")
    }

    private static func kilohertz(_ rate: UInt32) -> String {
        String(format: "%g kHz", Double(rate) / 1000)
    }
}

struct TransportRow: View {
    @Environment(Library.self) private var library
    @State private var skipped = 0

    var body: some View {
        let shuffled = library.snapshot?.shuffle ?? false
        let repeating = library.repeatMode
        GlassEffectContainer(spacing: 24) {
            HStack(spacing: 0) {
                Button("Shuffle", systemImage: "shuffle") { library.setShuffle(!shuffled) }
                    .foregroundStyle(shuffled ? Color.worn : .secondary)
                    .accessibilityValue(shuffled ? "On" : "Off")
                    .touchTarget()
                    .frame(maxWidth: .infinity)
                Button("Previous", systemImage: "backward.fill") {
                    skipped += 1
                    library.previous()
                }
                .font(.title)
                .touchTarget()
                .frame(maxWidth: .infinity)
                PlayButton(transport: library.transport, large: true)
                Button("Next", systemImage: "forward.fill") {
                    skipped += 1
                    library.next()
                }
                .font(.title)
                .touchTarget()
                .frame(maxWidth: .infinity)
                Button("Repeat", systemImage: repeating.symbol) { library.setRepeat(repeating.following) }
                    .foregroundStyle(repeating == .off ? .secondary : Color.worn)
                    .accessibilityValue(repeating.spoken)
                    .touchTarget()
                    .frame(maxWidth: .infinity)
            }
            .labelStyle(.iconOnly)
            .buttonStyle(.plain)
        }
        .disabled(!library.loaded)
        .sensoryFeedback(.impact(weight: .light), trigger: skipped)
        .sensoryFeedback(.selection, trigger: shuffled)
        .sensoryFeedback(.selection, trigger: repeating)
    }
}

struct SleepCountdown: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation

    var body: some View {
        if let text = Self.text(library.snapshot?.sleep, remaining: library.clock.sleepRemainingMs) {
            Button {
                navigation.sleepTimer = true
            } label: {
                Label(text, systemImage: "moon.zzz.fill")
                    .font(.footnote.monospacedDigit())
                    .foregroundStyle(.secondary)
            }
            .buttonStyle(.plain)
            .accessibilityHint("Opens the sleep timer")
        }
    }

    static func text(_ timer: SleepTimer?, remaining: UInt64?) -> String? {
        switch timer {
        case .after: "Pausing in \(clock((remaining ?? 0) + 999))"
        case .endOfTrack: "Pauses at the end of this song"
        case .endOfQueue: "Pauses at the end of the queue"
        case nil: nil
        }
    }
}

struct VolumeRow: View {
    private static let restingHeight: CGFloat = 7
    private static let grabbedHeight: CGFloat = 14
    private static let touchHeight: CGFloat = 34
    private static let step: Float = 1 / 16

    @State private var volume = SystemVolumeLevel()
    @State private var dragging: Float?
    @State private var grabbedAt: Float?

    var body: some View {
        let shown = dragging ?? volume.level
        let grabbed = dragging != nil

        HStack(spacing: 12) {
            Image(systemName: "speaker.fill").foregroundStyle(.secondary).accessibilityHidden(true)
            GeometryReader { space in
                let width = max(space.size.width, 1)
                Capsule()
                    .fill(.primary.opacity(0.16))
                    .overlay(alignment: .leading) {
                        Rectangle()
                            .fill(.primary.opacity(grabbed ? 0.95 : 0.7))
                            .frame(width: width * CGFloat(shown))
                    }
                    .clipShape(Capsule())
                    .frame(height: grabbed ? Self.grabbedHeight : Self.restingHeight)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .contentShape(Rectangle())
                    .gesture(drag(across: width, from: shown))
            }
            .frame(height: Self.touchHeight)
            .background { VolumeDriver(volume: volume).allowsHitTesting(false) }
            .motion(.spring(duration: 0.3, bounce: 0.3), value: grabbed)
            Image(systemName: "speaker.wave.3.fill").foregroundStyle(.secondary).accessibilityHidden(true)
        }
        .accessibilityElement()
        .accessibilityLabel("Volume")
        .accessibilityValue(Text(Double(shown), format: .percent.precision(.fractionLength(0))))
        .accessibilityAdjustableAction { direction in
            let step = direction == .increment ? Self.step : -Self.step
            volume.set(min(max(shown + step, 0), 1))
        }
        .onAppear { volume.watch() }
        .onDisappear { volume.stopWatching() }
    }

    private func drag(across width: CGFloat, from shown: Float) -> some Gesture {
        DragGesture(minimumDistance: 0)
            .onChanged { change in
                let start = grabbedAt ?? shown
                if grabbedAt == nil { grabbedAt = start }
                let moved = Float(change.translation.width / width)
                let wanted = min(max(start + moved, 0), 1)

                dragging = wanted
                volume.set(wanted)
            }
            .onEnded { _ in
                grabbedAt = nil
                dragging = nil
            }
    }
}

@MainActor
@Observable
final class SystemVolumeLevel {
    private(set) var level: Float = AVAudioSession.sharedInstance().outputVolume
    @ObservationIgnored weak var driver: MPVolumeView?
    @ObservationIgnored private var watching: NSKeyValueObservation?

    func watch() {
        guard watching == nil else { return }
        level = AVAudioSession.sharedInstance().outputVolume
        watching = Self.observing { [weak self] heard in self?.level = heard }
    }

    func stopWatching() {
        watching?.invalidate()
        watching = nil
    }

    func set(_ wanted: Float) {
        level = wanted
        guard let slider = driver.flatMap(Self.slider(in:)) else { return }
        slider.setValue(wanted, animated: false)
        slider.sendActions(for: .valueChanged)
    }

    nonisolated private static func observing(
        _ heard: @escaping @MainActor @Sendable (Float) -> Void
    ) -> NSKeyValueObservation {
        AVAudioSession.sharedInstance().observe(\.outputVolume, options: [.new]) { _, change in
            guard let value = change.newValue else { return }
            Task { @MainActor in heard(value) }
        }
    }

    private static func slider(in view: UIView) -> UISlider? {
        for child in view.subviews {
            if let slider = child as? UISlider { return slider }
            if let nested = slider(in: child) { return nested }
        }
        return nil
    }
}

struct VolumeDriver: UIViewRepresentable {
    private static let barelyShown: CGFloat = 0.01

    let volume: SystemVolumeLevel

    func makeUIView(context: Context) -> MPVolumeView {
        let driver = MPVolumeView(frame: .zero)
        driver.setValue(false, forKey: "showsRouteButton")
        driver.alpha = Self.barelyShown
        driver.isUserInteractionEnabled = false
        volume.driver = driver
        return driver
    }

    func updateUIView(_ uiView: MPVolumeView, context: Context) {
        volume.driver = uiView
    }
}

struct PlayerModes: View {
    let mode: PlayerMode
    let choose: (PlayerMode) -> Void
    @State private var switched = 0

    var body: some View {
        GlassEffectContainer(spacing: 20) {
            HStack(spacing: 20) {
                toggle(.lyrics, title: "Lyrics", symbol: "quote.bubble")
                RoutePicker()
                    .frame(width: 48, height: 48)
                    .glassEffect(.regular.interactive(), in: .circle)
                    .accessibilityLabel("AirPlay")
                toggle(.queue, title: "Up next", symbol: "list.bullet")
            }
        }
        .sensoryFeedback(.selection, trigger: switched)
    }

    private func toggle(_ target: PlayerMode, title: String, symbol: String) -> some View {
        let active = mode == target
        return Button(title, systemImage: symbol) {
            switched += 1
            choose(active ? .artwork : target)
        }
        .labelStyle(.iconOnly)
        .font(.title3)
        .frame(width: 48, height: 48)
        .foregroundStyle(active ? Color.wornInk : Color.primary)
        .glassEffect(active ? .regular.tint(Color.worn).interactive() : .regular.interactive(), in: .circle)
        .buttonStyle(.plain)
        .accessibilityAddTraits(active ? .isSelected : [])
    }
}

struct PlayerMoreMenu: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    let track: Track?
    let addToPlaylist: (Track) -> Void

    var body: some View {
        Menu("More", systemImage: "ellipsis") {
            if let track {
                Section {
                    Button("Add to playlist", systemImage: "text.badge.plus") { addToPlaylist(track) }
                    Button("Song details", systemImage: "info.circle") { navigation.open(.inspector(track)) }
                    Button("Analyse", systemImage: "waveform.path.ecg") { navigation.open(.analysis(track)) }
                }
            }
            Section {
                Button("Sleep timer", systemImage: library.snapshot?.sleep == nil ? "moon.zzz" : "moon.zzz.fill") {
                    navigation.sleepTimer = true
                }
                Button("Equaliser", systemImage: "slider.vertical.3") {
                    navigation.tab = .settings
                    navigation.open(.sound)
                }
                Button("Visualizer", systemImage: "waveform") { navigation.open(.visualizer) }
                Button("Output", systemImage: "hifispeaker") {
                    navigation.tab = .settings
                    navigation.open(.output)
                }
            }
        }
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
    @State private var clearing = false
    @State private var saving = false

    var body: some View {
        QueueList(inPlayer: false)
            .navigationTitle("Queue")
            .refreshable { await library.rereadQueue() }
            .toolbar {
                if !library.queue.isEmpty {
                    ToolbarItem(placement: .topBarTrailing) {
                        Button("Save as playlist", systemImage: "text.badge.plus") { saving = true }
                    }
                    ToolbarItem(placement: .topBarTrailing) {
                        Button("Clear", systemImage: "trash") { clearing = true }
                            .disabled(library.queue.count <= 1)
                    }
                    ToolbarItem(placement: .topBarTrailing) { EditButton() }
                }
            }
            .alert("Save queue as playlist", isPresented: $saving) {
                PlaylistNameField(confirm: "Save") { library.features.createPlaylist($0, with: library.queue) }
            }
            .confirmationDialog("Clear the queue?", isPresented: $clearing, titleVisibility: .visible) {
                Button("Clear queue", role: .destructive) { library.clearQueue() }
            } message: {
                Text("Songs up next and in your history are removed. The playing song keeps playing.")
            }
    }
}

struct EmptyQueue: View {
    @Environment(Navigation.self) private var navigation

    var body: some View {
        ContentUnavailableView {
            Label("Your queue is empty", systemImage: "list.bullet")
        } description: {
            Text("Songs you play, play next or add to the queue appear here.")
        } actions: {
            Button("Browse songs") { navigation.tab = .songs }
                .buttonStyle(.glass)
        }
        .listRowSeparator(.hidden)
        .listRowBackground(Color.clear)
    }
}

struct UpNextList: View {
    var body: some View {
        QueueList(inPlayer: true)
    }
}

struct QueueList: View {
    private static let playingRow = "playing"

    @Environment(Library.self) private var library
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    let inPlayer: Bool

    var body: some View {
        ScrollViewReader { scroller in
            List {
                if !inPlayer { HandoffRows().themedRows() }
                QueueSections(inPlayer: inPlayer, playingRowId: Self.playingRow)
                    .listRowBackground(inPlayer || AppearanceState.shared.themedBackground ? Color.clear : nil)
            }
            .listStyle(.plain)
            .scrollContentBackground(inPlayer ? .hidden : .automatic)
            .onAppear { scroller.scrollTo(Self.playingRow, anchor: .top) }
            .onChange(of: library.currentQueueId) {
                withAnimation(reduceMotion ? nil : .smooth) { scroller.scrollTo(Self.playingRow, anchor: .top) }
            }
            .onChange(of: library.queue.isEmpty) {
                scroller.scrollTo(Self.playingRow, anchor: .top)
            }
        }
    }
}

struct QueueSections: View {
    private static let heardFaded = 0.55

    @Environment(Library.self) private var library
    let inPlayer: Bool
    let playingRowId: String
    @State private var edited = 0

    var body: some View {
        let rows = library.queue
        let playing = library.playingQueueRow
        Group {
            if rows.isEmpty, inPlayer {
                Text("Your queue is empty").foregroundStyle(.secondary)
            } else if rows.isEmpty {
                EmptyQueue()
            } else if let playing {
                let parts = QueueParts(rows: rows.count, playing: playing, next: library.playingNext)
                if playing > 0 {
                    Section {
                        QueueHeading(title: "History")
                        rowsOf(rows, parts.history).opacity(Self.heardFaded)
                    }
                }
                Section {
                    QueueHeading(title: "Now playing").id(playingRowId)
                    QueueRow(row: rows[playing], edited: $edited)
                }
                if !parts.next.isEmpty {
                    Section {
                        QueueHeading(title: "Playing next")
                        rowsOf(rows, parts.next)
                    }
                }
                Section {
                    QueueHeading(title: parts.next.isEmpty ? "Up next" : "Continue playing")
                    if parts.rest.isEmpty {
                        Text("Nothing more to play").foregroundStyle(.secondary)
                    } else {
                        rowsOf(rows, parts.rest)
                    }
                }
            } else {
                Section {
                    QueueHeading(title: "Up next")
                    rowsOf(rows, 0..<rows.count)
                }
            }
        }
        .sensoryFeedback(.selection, trigger: edited)
    }

    private func rowsOf(_ rows: [Track], _ range: Range<Int>) -> some View {
        ForEach(Array(rows[range]), id: \.queueId) { row in
            QueueRow(row: row, edited: $edited)
        }
        .onMove { offsets, destination in
            guard let moved = offsets.first else { return }
            edited += 1
            library.moveInQueue(from: range.lowerBound + moved, to: range.lowerBound + destination)
        }
        .onDelete { offsets in
            edited += 1
            for offset in offsets.sorted(by: >) { library.removeFromQueue(rows[range.lowerBound + offset]) }
        }
    }
}

struct QueueHeading: View {
    let title: String

    var body: some View {
        Text(title)
            .font(.subheadline.weight(.semibold))
            .foregroundStyle(.secondary)
            .listRowSeparator(.hidden)
            .listRowBackground(Color.clear)
            .accessibilityAddTraits(.isHeader)
    }
}

struct QueueParts {
    let history: Range<Int>
    let next: Range<Int>
    let rest: Range<Int>

    init(rows: Int, playing: Int, next asked: Range<Int>?) {
        let after = min(playing + 1, rows)
        history = 0..<min(playing, rows)
        let nextEnd = asked.map { min(max($0.upperBound, after), rows) } ?? after
        next = after..<nextEnd
        rest = nextEnd..<rows
    }
}

struct QueueRow: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    let row: Track
    @Binding var edited: Int
    @State private var choosingPlaylist = false

    var body: some View {
        let favourite = library.isFavourite(row)
        TrackRow(track: row, current: row.queueId == library.currentQueueId, ownsMenus: false) {
            library.jump(to: row)
        }
        .swipeActions(edge: .trailing) {
            Button("Remove from queue", systemImage: "minus.circle", role: .destructive) {
                edited += 1
                library.removeFromQueue(row)
            }
        }
        .contextMenu {
            Button("Remove from queue", systemImage: "minus.circle", role: .destructive) {
                edited += 1
                library.removeFromQueue(row)
            }
            Button(favourite ? "Remove from favourites" : "Add to favourites", systemImage: favourite ? "heart.slash" : "heart") {
                library.setFavourite(row, !favourite)
            }
            Button("Add to playlist", systemImage: "text.badge.plus") { choosingPlaylist = true }
            Button("Song details", systemImage: "info.circle") { navigation.open(.inspector(row)) }
        }
        .sheet(isPresented: $choosingPlaylist) { PlaylistPicker(tracks: [row]) }
    }
}

struct HandoffRows: View {
    @Environment(Library.self) private var library

    var body: some View {
        ForEach(library.features.syncHandoffs, id: \.id) { handoff in
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
    private static let minutes: [UInt32] = [5, 15, 30, 45, 60, 90]

    @Environment(Library.self) private var library
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        let timer = library.snapshot?.sleep
        NavigationStack {
            List {
                Section {
                    SleepTimerState()
                }
                Section {
                    ForEach(Self.minutes, id: \.self) { minutes in
                        Button("\(minutes) minutes") { set(.after(seconds: minutes * 60)) }
                    }
                    Button("End of song") { set(.endOfTrack) }
                    Button("End of queue") { set(.endOfQueue) }
                }
                if timer != nil {
                    Button("Cancel timer", role: .destructive) { set(nil) }
                }
            }
            .navigationTitle("Sleep timer")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button(role: .close) { dismiss() }
                }
            }
        }
    }

    private func set(_ timer: SleepTimer?) {
        library.request { try $0.setSleep(timer: timer) }
        dismiss()
    }
}

struct SleepTimerState: View {
    @Environment(Library.self) private var library

    var body: some View {
        if let text = SleepCountdown.text(library.snapshot?.sleep, remaining: library.clock.sleepRemainingMs) {
            Text(text).font(.headline.monospacedDigit())
        } else {
            Text("No timer set").foregroundStyle(.secondary)
        }
    }
}
