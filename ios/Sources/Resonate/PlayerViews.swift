import AVKit
import MediaPlayer
import ResonateLogic
import ResonateNative
import SwiftUI

struct MiniPlayer: View {
    private static let swipeDistance: CGFloat = 40
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
    @State private var fit = InlineFit.unmeasured

    private enum InlineFit {
        private static let titledFrom: CGFloat = 150
        private static let skipsFrom: CGFloat = 200

        case unmeasured
        case coverAndPlay
        case titled
        case skipping

        init(width: CGFloat) {
            self =
                width >= Self.skipsFrom ? .skipping
                : width >= Self.titledFrom ? .titled
                : .coverAndPlay
        }
    }

    var body: some View {
        let track = library.playingTrack
        let expanded = placement != .inline
        let titled = expanded || fit != .coverAndPlay
        let skips = expanded || fit == .skipping
        HStack(spacing: expanded ? 12 : 8) {
            ArtworkView(track: track, corner: expanded ? 8 : 7, handsOver: true)
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
                .motion(Handover.animation, value: track?.id)
            } else {
                Spacer(minLength: 0)
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
        .transaction(value: placement) { $0.animation = nil }
        .transaction(value: fit) { $0.animation = nil }
        .onGeometryChange(for: InlineFit.self) { InlineFit(width: $0.size.width) } action: { fit = $0 }
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
        .touchTarget()
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
    private static let reshapedWithin: CFTimeInterval = 0.8

    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    @Environment(\.dismiss) private var dismiss
    @Environment(\.dynamicTypeSize) private var typeSize
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var mode = PlayerMode.artwork
    @State private var settled: PlayerMode?
    @State private var reshaping = false
    @State private var picked: PickedTracks?
    @State private var savingQueue = false
    @State private var clearingQueue = false

    var body: some View {
        let track = library.playingTrack
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
            .background(BackdropView(track: track, resting: reshaping))
            .safeAreaInset(edge: .top) { StatusBanner() }
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button("Close", systemImage: "chevron.down") { dismiss() }
                }
                ToolbarItem(placement: .topBarTrailing) {
                    PlayerMoreMenu(
                        track: track,
                        addToPlaylist: { picked = PickedTracks(tracks: [$0]) },
                        saveQueue: { savingQueue = true },
                        clearQueue: { clearingQueue = true }
                    )
                }
            }
            .sheet(item: $picked) { PlaylistPicker(tracks: $0.tracks) }
            .alert("Save queue as playlist", isPresented: $savingQueue) {
                PlaylistNameField(confirm: "Save") { library.features.createPlaylist($0, with: library.queue) }
            }
            .confirmationDialog("Clear the queue?", isPresented: $clearingQueue, titleVisibility: .visible) {
                Button("Clear queue", role: .destructive) { library.clearQueue() }
            } message: {
                Text("Songs up next and in your history are removed. The playing song keeps playing.")
            }
            .modifier(DeleteSongsConfirmation(insidePlayer: true))
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
        FrameRateBoost.shared.boost(for: Self.reshapedWithin)
        guard coverMoves else {
            withAnimation(Self.arriving) {
                mode = target
                settled = target
            }
            return
        }
        reshaping = true
        withAnimation(Self.reshaping, completionCriteria: .removed) {
            mode = target
        } completion: {
            reshaping = false
        }
        if target == .artwork {
            withAnimation(Self.leaving) { settled = nil }
        } else {
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
    private static let corner: CGFloat = 24
    private static let settlesOnItsSlot: Animation = .smooth(duration: 0.25)

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
        ArtworkView(track: track, corner: Self.corner, handsOver: true)
            .frame(width: drawn, height: drawn)
            .background { CoverShadow(corner: Self.corner) }
            .scaleEffect(resting)
            .motion(.spring(duration: 0.5, bounce: 0.25), value: playing)
            .scaleEffect(slot.width / drawn)
            .offset(x: slot.midX - drawn / 2, y: slot.midY - drawn / 2)
            .transaction(value: slot) { moved in
                if moved.animation == nil, !reduceMotion { moved.animation = Self.settlesOnItsSlot }
            }
            .motion(Handover.animation, value: track?.id)
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            .onChange(of: slot, initial: true) { _, now in
                if large, now.width > 0, now.width != edge { edge = now.width }
            }
    }
}

struct CoverShadow: View {
    private static let blur: CGFloat = 28
    private static let drop: CGFloat = 16
    private static let darkness = 0.3
    private static let reach = blur * 2 + drop
    private static let drawnAtScale: CGFloat = 1

    let corner: CGFloat

    var body: some View {
        Canvas { context, size in
            let cover = CGRect(origin: .zero, size: size).insetBy(dx: Self.reach, dy: Self.reach)
            guard cover.width > 0, cover.height > 0 else { return }

            context.addFilter(.shadow(color: .black.opacity(Self.darkness), radius: Self.blur, y: Self.drop, options: .shadowOnly))
            context.fill(RoundedRectangle(cornerRadius: corner, style: .continuous).path(in: cover), with: .color(.black))
        }
        .environment(\.displayScale, Self.drawnAtScale)
        .padding(-Self.reach)
        .allowsHitTesting(false)
        .accessibilityHidden(true)
    }
}

struct PlayerTitles: View {
    let track: Track?

    var body: some View {
        HStack(alignment: .center, spacing: 12) {
            words
                .contentTransition(.opacity)
                .motion(Handover.animation, value: track?.id)
            Spacer(minLength: 0)
            if let track { FavouriteButton(track: track).font(.title2) }
        }
    }

    private var words: some View {
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
                        .nowPlayingInk()
                }
                if !track.album.isEmpty {
                    SongMenu(track: track) {
                        Text(track.album)
                            .font(.subheadline)
                            .nowPlayingInk()
                    }
                }
            } else {
                Text("Nothing playing")
                    .font(.title2.weight(.bold))
                    .lineLimit(2)
            }
        }
        .lineLimit(1)
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
        .accessibilityHint("Shows the album, artist, song details, tags, analysis and a link to share")
    }
}

struct SongDestinations: View {
    let track: Track

    var body: some View {
        SongPages(track: track, lyrics: false)
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
            words
                .contentTransition(.opacity)
                .motion(Handover.animation, value: track?.id)
            Spacer(minLength: 0)
            if let track { FavouriteButton(track: track).font(.title3) }
        }
    }

    @ViewBuilder
    private var words: some View {
        if let track {
            SongMenu(track: track) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(track.shownTitle).font(.headline).foregroundStyle(Color.primary).lineLimit(1)
                    Text(track.shownArtist).font(.subheadline).nowPlayingInk().lineLimit(1)
                }
                .accessibilityElement(children: .combine)
            }
        } else {
            Text("Nothing playing").font(.headline).lineLimit(1)
        }
    }
}

struct PlayerScrubber: View {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    private static let nearEnoughMs: Double = 1500
    private static let holdAtMost: Duration = .seconds(4)
    private static let restingHeight: CGFloat = 7
    private static let grabbedHeight: CGFloat = 14
    private static let touchHeight: CGFloat = 28
    private static let stepMs: Double = 10_000
    private static let slightestMove: CGFloat = 2
    private static let flowsWithin: ClosedRange<CFTimeInterval> = 0.05...0.6
    private static let flowsAcrossMs: Double = 1500
    private static let settles: Animation = .easeOut(duration: 0.15)

    @Environment(Library.self) private var library
    @State private var dragging: Double?
    @State private var grabbedAt: Double?
    @State private var held: Double?
    @State private var grabs = 0
    @State private var flowing: Double = 0
    @State private var lastReading = LastReading()
    @GestureState private var touching = false

    private final class LastReading {
        var at: CFTimeInterval = 0
        var flow: Animation?
    }

    var body: some View {
        let timing = library.clock
        let duration = Double(max(timing.durationMs, 1))
        let live = Double(timing.positionMs)
        let shown = min(dragging ?? held ?? live, duration)
        let drawn = min(dragging ?? held ?? flowing, duration)
        let grabbed = dragging != nil

        VStack(spacing: 6) {
            GeometryReader { space in
                let width = max(space.size.width, 1)
                Capsule()
                    .fill(.primary.opacity(0.16))
                    .overlay(alignment: .leading) {
                        Rectangle()
                            .fill(.primary.opacity(grabbed ? 0.95 : 0.7))
                            .frame(width: width * drawn / duration)
                            .animation(lastReading.flow, value: flowing)
                    }
                    .clipShape(Capsule())
                    .frame(height: grabbed ? Self.grabbedHeight : Self.restingHeight)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .contentShape(Rectangle())
                    .gesture(drag(across: width, of: duration, from: shown))
            }
            .frame(height: Self.touchHeight)
            .motion(.spring(duration: 0.3, bounce: 0.3), value: grabbed)
            .accessibilityElement()
            .accessibilityLabel("Song position")
            .accessibilityValue("\(clock(UInt64(shown))) of \(clock(timing.durationMs))")
            .accessibilityAdjustableAction { direction in
                let step = direction == .increment ? Self.stepMs : -Self.stepMs
                commit(min(max(shown + step, 0), duration))
            }
            HStack {
                Text(clock(UInt64(shown)))
                    .accessibilityHidden(true)
                Spacer()
                if let waiting = library.waitingForADevice {
                    WaitingLine(text: waiting)
                } else {
                    QualityLine(track: library.playingTrack)
                }
                Spacer()
                Text("-" + clock(UInt64(max(duration - shown, 0))))
                    .accessibilityHidden(true)
            }
            .font(.caption.monospacedDigit())
            .nowPlayingInk(grabbed ? 1 : NowPlayingInk.secondary)
            .motion(.smooth, value: grabbed)
        }
        .allowsHitTesting(library.loaded)
        .sensoryFeedback(.impact(weight: .light), trigger: grabs)
        .onChange(of: touching) { _, now in
            guard !now else { return }
            grabbedAt = nil
            dragging = nil
        }
        .onChange(of: timing.positionMs, initial: true) { old, now in
            if let target = held, abs(Double(now) - target) < Self.nearEnoughMs { held = nil }
            flow(from: Double(old), to: Double(now))
        }
        .onChange(of: library.transport) {
            lastReading.flow = reduceMotion ? nil : Self.settles
            flowing = held ?? Double(library.clock.positionMs)
        }
        .task(id: held) {
            guard held != nil else { return }
            try? await Task.sleep(for: Self.holdAtMost)
            if !Task.isCancelled { held = nil }
        }
    }

    private func drag(across width: CGFloat, of duration: Double, from shown: Double) -> some Gesture {
        DragGesture(minimumDistance: 0)
            .updating($touching) { _, held, _ in held = true }
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
        lastReading.flow = nil
        flowing = target
        library.seek(to: UInt64(target))
    }

    private func flow(from old: Double, to now: Double) {
        let at = CACurrentMediaTime()
        let first = lastReading.at == 0
        let elapsed = min(max(at - lastReading.at, Self.flowsWithin.lowerBound), Self.flowsWithin.upperBound)
        lastReading.at = at
        let stepped = now - old
        let free = dragging == nil && held == nil
        let flows = free && !first && !reduceMotion && library.transport == .playing && stepped >= 0 && stepped < Self.flowsAcrossMs
        lastReading.flow = flows ? .linear(duration: elapsed) : nil
        flowing = held ?? (flows ? now + elapsed * 1000 : now)
    }
}

struct WaitingLine: View {
    let text: String

    var body: some View {
        Label(text, systemImage: "speaker.slash")
            .font(.caption2.weight(.semibold))
            .nowPlayingInk()
            .lineLimit(2)
            .multilineTextAlignment(.center)
            .minimumScaleFactor(0.7)
            .padding(.horizontal, 7)
            .padding(.vertical, 2)
            .background(.quaternary, in: .capsule)
    }
}

struct QualityLine: View {
    @Environment(Library.self) private var library
    let track: Track?
    @State private var lastOutput: UInt32 = 0

    var body: some View {
        let reading = library.clock.outputRate
        let output = reading > 0 ? reading : lastOutput
        let line = Self.line(track, output: output)
        if !line.trimmingCharacters(in: .whitespaces).isEmpty {
            Text(line)
                .font(.caption2.weight(.semibold))
                .nowPlayingInk()
                .lineLimit(1)
                .minimumScaleFactor(0.7)
                .padding(.horizontal, 7)
                .padding(.vertical, 2)
                .background(.quaternary, in: .capsule)
                .onChange(of: reading, initial: true) { _, now in
                    if now > 0 { lastOutput = now }
                }
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

struct NowPlayingInk: ViewModifier {
    static let secondary = 0.95

    @Environment(\.colorSchemeContrast) private var contrast
    @Environment(\.accessibilityReduceTransparency) private var reduceTransparency
    let level: Double

    func body(content: Content) -> some View {
        content.foregroundStyle(Color.primary.opacity(contrast == .increased || reduceTransparency ? 1 : level))
    }
}

private struct LiftedFieldKey: EnvironmentKey {
    static let defaultValue = false
}

extension EnvironmentValues {
    var liftedField: Bool {
        get { self[LiftedFieldKey.self] }
        set { self[LiftedFieldKey.self] = newValue }
    }
}

struct SecondaryInk: ViewModifier {
    @Environment(\.liftedField) private var lifted

    func body(content: Content) -> some View {
        if lifted {
            content.nowPlayingInk()
        } else {
            content.foregroundStyle(.secondary)
        }
    }
}

extension View {
    func nowPlayingInk(_ level: Double = NowPlayingInk.secondary) -> some View {
        modifier(NowPlayingInk(level: level))
    }

    func secondaryInk() -> some View {
        modifier(SecondaryInk())
    }
}

private struct ModeMark: ViewModifier {
    let active: Bool

    func body(content: Content) -> some View {
        content.overlay(alignment: .bottom) {
            Circle()
                .fill(Color.worn)
                .frame(width: 5, height: 5)
                .offset(y: 8)
                .opacity(active ? 1 : 0)
                .accessibilityHidden(true)
        }
    }
}

extension View {
    fileprivate func modeMark(_ active: Bool) -> some View {
        modifier(ModeMark(active: active))
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
                    .modeMark(shuffled)
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
                    .modeMark(repeating != .off)
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
                    .nowPlayingInk()
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
    private static let driverMountsAfter: Duration = .milliseconds(600)

    @State private var volume = SystemVolumeLevel()
    @State private var dragging: Float?
    @State private var grabbedAt: Float?
    @State private var driven = false
    @GestureState private var touching = false

    var body: some View {
        let shown = dragging ?? volume.level
        let grabbed = dragging != nil

        HStack(spacing: 12) {
            Image(systemName: "speaker.fill").nowPlayingInk().accessibilityHidden(true)
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
            .background {
                if driven { VolumeDriver(volume: volume).allowsHitTesting(false) }
            }
            .motion(.spring(duration: 0.3, bounce: 0.3), value: grabbed)
            Image(systemName: "speaker.wave.3.fill").nowPlayingInk().accessibilityHidden(true)
        }
        .accessibilityElement()
        .accessibilityLabel("Volume")
        .accessibilityValue(Text(Double(shown), format: .percent.precision(.fractionLength(0))))
        .accessibilityAdjustableAction { direction in
            let step = direction == .increment ? Self.step : -Self.step
            driven = true
            volume.set(min(max(shown + step, 0), 1))
        }
        .onChange(of: touching) { _, now in
            guard !now else { return }
            grabbedAt = nil
            dragging = nil
        }
        .onAppear { volume.watch() }
        .onDisappear { volume.stopWatching() }
        .task {
            try? await Task.sleep(for: Self.driverMountsAfter)
            driven = true
        }
    }

    private func drag(across width: CGFloat, from shown: Float) -> some Gesture {
        DragGesture(minimumDistance: 0)
            .updating($touching) { _, held, _ in held = true }
            .onChanged { change in
                let start = grabbedAt ?? shown
                if grabbedAt == nil { grabbedAt = start }
                driven = true
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
    private static let seedTries = 10
    private static let seedEvery: Duration = .milliseconds(100)

    private static var lastShown: Float = 0

    private(set) var level: Float = SystemVolumeLevel.lastShown
    @ObservationIgnored weak var driver: MPVolumeView?
    @ObservationIgnored private var watching: NSKeyValueObservation?
    @ObservationIgnored private var heardSinceWatching = false
    @ObservationIgnored private var sessionAnswered = false
    @ObservationIgnored private var watchAsked = false
    @ObservationIgnored private var watches = 0

    func watch() {
        guard !watchAsked else { return }
        watchAsked = true
        heardSinceWatching = false
        sessionAnswered = false
        watches += 1
        let watch = watches
        let hearing: @MainActor @Sendable (Float) -> Void = { [weak self] heard in self?.hear(heard) }
        Task {
            let (observation, current) = await Work.inSession {
                (Self.observing(hearing), AVAudioSession.sharedInstance().outputVolume)
            }
            guard watch == watches else {
                observation.invalidate()
                return
            }
            watching = observation
            guard !heardSinceWatching, current > 0 else { return }
            sessionAnswered = true
            show(current)
        }
    }

    func seed(from driver: MPVolumeView) {
        Task {
            for _ in 0..<Self.seedTries {
                driver.layoutIfNeeded()
                if let slider = Self.slider(in: driver) {
                    if watchAsked, !heardSinceWatching, !sessionAnswered { show(slider.value) }
                    return
                }
                try? await Task.sleep(for: Self.seedEvery)
            }
        }
    }

    private func hear(_ heard: Float) {
        heardSinceWatching = true
        show(heard)
    }

    private func show(_ shown: Float) {
        level = shown
        Self.lastShown = shown
    }

    func stopWatching() {
        watches += 1
        watchAsked = false
        watching?.invalidate()
        watching = nil
    }

    func set(_ wanted: Float) {
        heardSinceWatching = true
        show(wanted)
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
        volume.seed(from: driver)
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
        return Button {
            switched += 1
            choose(active ? .artwork : target)
        } label: {
            Label(title, systemImage: symbol)
                .labelStyle(.iconOnly)
                .font(.title3)
                .frame(width: 48, height: 48)
                .contentShape(.circle)
        }
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
    let saveQueue: () -> Void
    let clearQueue: () -> Void

    var body: some View {
        Menu("More", systemImage: "ellipsis") {
            if let track {
                Section {
                    Button("Add to playlist", systemImage: "text.badge.plus") { addToPlaylist(track) }
                    Button("Song details", systemImage: "info.circle") { navigation.open(.inspector(track)) }
                    Button("Analyse", systemImage: "waveform.path.ecg") { navigation.open(.analysis(track)) }
                }
            }
            if !library.queue.isEmpty {
                Section("Queue") {
                    Button("Save queue as playlist", systemImage: "text.badge.plus", action: saveQueue)
                    Button("Clear queue", systemImage: "trash", role: .destructive, action: clearQueue)
                        .disabled(library.queue.count <= 1)
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

@MainActor
enum PlayerWarmup {
    private static let afterLaunch: Duration = .seconds(2)

    static func warm() async {
        try? await Task.sleep(for: afterLaunch)
        guard !Task.isCancelled else { return }
        let picker = AVRoutePickerView()
        picker.prioritizesVideoDevices = false
        let warmed: [UIView] = [picker, MPVolumeView(frame: .zero)]
        for view in warmed { view.layoutIfNeeded() }
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

struct UpNextList: View {
    private static let playingRow = "playing"

    @Environment(Library.self) private var library
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var onScreen = false

    var body: some View {
        ScrollViewReader { scroller in
            List {
                QueueSections(playingRowId: Self.playingRow)
                    .listRowBackground(Color.clear)
            }
            .listStyle(.plain)
            .scrollContentBackground(.hidden)
            .environment(\.liftedField, true)
            .onAppear {
                onScreen = true
                scroller.scrollTo(Self.playingRow, anchor: .top)
            }
            .onDisappear { onScreen = false }
            .onChange(of: library.currentQueueId) {
                guard onScreen else { return }
                withAnimation(reduceMotion ? nil : .smooth) { scroller.scrollTo(Self.playingRow, anchor: .top) }
            }
            .onChange(of: library.queue.isEmpty) {
                guard onScreen else { return }
                scroller.scrollTo(Self.playingRow, anchor: .top)
            }
        }
    }
}

struct QueueSections: View {
    private static let heardFaded = 0.55
    private static let heardAtFirst = 50
    private static let aheadAtFirst = 200
    private static let widenedBy = 200

    @Environment(Library.self) private var library
    let playingRowId: String
    @State private var edited = 0
    @State private var heard = Self.heardAtFirst
    @State private var ahead = Self.aheadAtFirst

    var body: some View {
        let rows = library.queue
        let playing = library.playingQueueRow
        Group {
            if rows.isEmpty {
                Text("Your queue is empty").secondaryInk()
            } else if let playing {
                let parts = QueueParts(
                    rows: rows.count,
                    playing: playing,
                    next: library.playingNext,
                    heardAtMost: heard,
                    aheadAtMost: ahead
                )
                if playing > 0 {
                    Section {
                        QueueHeading(title: "History")
                        if parts.history.lowerBound > 0 {
                            moreRows(parts.history.lowerBound == 1 ? "1 earlier song" : "\(parts.history.lowerBound) earlier songs") {
                                heard += Self.widenedBy
                            }
                        }
                        rowsOf(rows, parts.history).opacity(Self.heardFaded)
                    }
                }
                Section {
                    QueueHeading(title: "Now playing").id(playingRowId)
                    QueueRow(row: rows[playing], current: true, edited: $edited)
                }
                if !parts.next.isEmpty {
                    Section {
                        QueueHeading(title: "Playing next")
                        rowsOf(rows, parts.next)
                    }
                }
                Section {
                    QueueHeading(title: parts.next.isEmpty ? "Up next" : "Continue playing")
                    if parts.rest.isEmpty, parts.beyond == 0 {
                        Text("Nothing more to play").secondaryInk()
                    } else {
                        rowsOf(rows, parts.rest)
                    }
                    if parts.beyond > 0 {
                        further(parts.beyond)
                    }
                }
            } else {
                let shown = min(rows.count, ahead)
                Section {
                    QueueHeading(title: "Up next")
                    rowsOf(rows, 0..<shown)
                    if rows.count > shown {
                        further(rows.count - shown)
                    }
                }
            }
        }
        .sensoryFeedback(.selection, trigger: edited)
    }

    private func further(_ count: Int) -> some View {
        moreRows(count == 1 ? "1 more song" : "\(count) more songs") { ahead += Self.widenedBy }
    }

    private func moreRows(_ title: String, _ widen: @escaping () -> Void) -> some View {
        Button(action: widen) {
            HStack {
                Text(title)
                Spacer()
                Text("Show").font(.footnote.weight(.semibold))
            }
            .secondaryInk()
            .contentShape(.rect)
        }
        .buttonStyle(.plain)
        .listRowBackground(Color.clear)
        .accessibilityHint("Shows more of the queue")
    }

    private func rowsOf(_ rows: [Track], _ range: Range<Int>) -> some View {
        ForEach(rows[range], id: \.queueId) { row in
            QueueRow(row: row, current: false, edited: $edited)
        }
        .onMove { offsets, destination in
            guard let moved = offsets.first else { return }
            edited += 1
            let move = RowMoves.queue(offset: moved, destination: destination, within: range.lowerBound)
            library.moveInQueue(from: move.from, to: move.to)
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
            .secondaryInk()
            .listRowSeparator(.hidden)
            .listRowBackground(Color.clear)
            .accessibilityAddTraits(.isHeader)
    }
}

struct QueueRow: View {
    @Environment(Library.self) private var library
    let row: Track
    let current: Bool
    @Binding var edited: Int
    @State private var choosingPlaylist = false

    var body: some View {
        let favourite = library.isFavourite(row)
        TrackRow(track: row, current: current, ownsMenus: false) {
            library.jump(to: row)
        }
        .swipeActions(edge: .trailing) {
            Button("Remove from queue", systemImage: "minus.circle", role: .destructive) {
                edited += 1
                library.removeFromQueue(row)
            }
            .tint(.red)
        }
        .contextMenu {
            Section {
                Button(favourite ? "Remove from favourites" : "Add to favourites", systemImage: favourite ? "heart.slash" : "heart") {
                    library.setFavourite(row, !favourite)
                }
                Button("Add to playlist", systemImage: "text.badge.plus") { choosingPlaylist = true }
            }
            Section {
                SongPages(track: row)
            }
            Section {
                Button("Remove from queue", systemImage: "minus.circle", role: .destructive) {
                    edited += 1
                    library.removeFromQueue(row)
                }
            }
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
