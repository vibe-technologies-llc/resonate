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
    @Environment(\.colorSchemeContrast) private var contrast
    @ScaledMetric(relativeTo: .subheadline) private var artworkEdge: CGFloat = 36
    @ScaledMetric(relativeTo: .subheadline) private var inlineArtworkEdge: CGFloat = 28
    let zoom: Namespace.ID
    @State private var skipped = 0
    @State private var fit = InlineFit.unmeasured
    @State private var swipedOpenFrom: CGPoint?

    private enum InlineFit {
        private static let titledFrom: CGFloat = 150

        case unmeasured
        case coverAndPlay
        case titled

        init(width: CGFloat) {
            self = width >= Self.titledFrom ? .titled : .coverAndPlay
        }
    }

    var body: some View {
        let track = library.playingTrack
        let expanded = placement != .inline
        let titled = expanded || fit != .coverAndPlay
        HStack(spacing: expanded ? 12 : 8) {
            ArtworkView(track: track, corner: expanded ? 8 : 7, handsOver: true)
                .frame(width: expanded ? artworkEdge : inlineArtworkEdge, height: expanded ? artworkEdge : inlineArtworkEdge)
                .matchedTransitionSource(id: PlayerTransition.source, in: zoom)
            if titled {
                VStack(alignment: .leading, spacing: 0) {
                    Text(track?.shownTitle ?? "")
                        .font(expanded ? .subheadline.weight(.semibold) : .footnote.weight(.semibold))
                        .lineLimit(1)
                    Text(track?.shownArtist ?? "")
                        .font(expanded ? .caption : .caption2)
                        .foregroundStyle(Color.primary.opacity(contrast == .increased ? 1 : Self.artistOpacity))
                        .lineLimit(1)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .contentTransition(reduceMotion ? .identity : .opacity)
                .handover(value: track?.pictureKey)
            } else {
                Spacer(minLength: 0)
            }
            HStack(spacing: expanded ? 12 : 0) {
                Button { skip(forward: false) } label: {
                    Label("player.previous", systemImage: "backward.fill").labelStyle(.iconOnly).touchTarget().buttonShape()
                }
                .accessibilityInputLabels([said("player.previous"), said("player.previous.input.song"), said("player.previous.input.back")])
                .modifier(RidesWithThePlayButton(shown: expanded))
                PlayButton(transport: library.transport)
                Button { skip(forward: true) } label: {
                    Label("player.next", systemImage: "forward.fill").labelStyle(.iconOnly).touchTarget().buttonShape()
                }
                .accessibilityInputLabels([said("player.next"), said("player.next.input.song"), said("player.next.input.skip")])
                .modifier(RidesWithThePlayButton(shown: expanded))
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
        .simultaneousGesture(swipes)
        .sensoryFeedback(.impact(weight: .light), trigger: skipped)
        .accessibilityElement(children: .contain)
        .accessibilityLabel(track.map { "\($0.shownTitle), \($0.shownArtist)" } ?? "")
        .accessibilityAddTraits(.isButton)
        .accessibilityAction(named: "player.open") { navigation.nowPlaying = true }
        .accessibilityAction(named: "player.previous") { skip(forward: false) }
        .accessibilityAction(named: "player.next") { skip(forward: true) }
    }

    private var swipes: some Gesture {
        DragGesture(minimumDistance: 24)
            .onChanged { drag in
                guard swipedOpenFrom != drag.startLocation,
                    -drag.translation.height > max(abs(drag.translation.width), Self.swipeDistance)
                else { return }
                swipedOpenFrom = drag.startLocation
                navigation.nowPlaying = true
            }
            .onEnded { drag in
                guard swipedOpenFrom != drag.startLocation,
                    abs(drag.translation.width) > max(abs(drag.translation.height), Self.swipeDistance)
                else { return }
                skip(forward: drag.translation.width < 0)
            }
    }

    private func skip(forward: Bool) {
        skipped += 1
        if forward { library.next() } else { library.previous() }
    }
}

private struct RidesWithThePlayButton: ViewModifier {
    let shown: Bool

    func body(content: Content) -> some View {
        content
            .opacity(shown ? 1 : 0)
            .motion(Arrival.animation, value: shown)
            .frame(width: shown ? nil : 0)
            .allowsHitTesting(shown)
            .accessibilityHidden(!shown)
    }
}

struct PlayButton: View {
    @Environment(Library.self) private var library
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @ScaledMetric(relativeTo: .largeTitle) private var largeEdge: CGFloat = 76
    @ScaledMetric(relativeTo: .title3) private var edge: CGFloat = 32
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
                .font(large ? .largeTitle.weight(.semibold) : .title3)
                .frame(width: large ? largeEdge : edge, height: large ? largeEdge : edge)
                .contentTransition(reduceMotion ? .identity : .symbolEffect(.replace))
                .motion(Flip.animation, value: playingish)
                .touchTarget()
        }
        .buttonStyle(.plain)
        .modifier(GlassWhen(enabled: large))
        .sensoryFeedback(.impact(weight: .medium), trigger: pressed)
        .keyboardShortcut(large ? KeyboardShortcut(.space, modifiers: []) : nil)
        .accessibilityLabel(said(playingish ? "player.pause" : "player.play"))
        .accessibilityInputLabels([
            said("player.play"), said("player.pause"), said("player.play.input.play-pause"), said("player.play.input.resume"),
        ])
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
    static let gutter: CGFloat = 24
    private static let reshaping: Animation = .spring(duration: 0.45, bounce: 0.15)
    private static let arriving = Arrival.animation
    private static let leaving: Animation = .easeOut(duration: 0.12)
    private static let reshapedWithin: CFTimeInterval = 0.8
    static let openedWithin: CFTimeInterval = 0.7
    private static let focusedHeightAtAccessibilitySizes: CGFloat = 360

    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    @Environment(\.dynamicTypeSize) private var typeSize
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var mode = PlayerMode.artwork
    @State private var settled: PlayerMode?
    @State private var rest = FieldRest()
    @State private var asks = PlayerAsks()

    var body: some View {
        let track = library.playingTrack
        @Bindable var asks = asks
        NavigationStack {
            ShapedScreen { shape in
                arranged(track, PlayerArrangement.of(shape))
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
            .background(NowPlayingField(track: track, rest: rest))
            .task { await open() }
            .safeAreaInset(edge: .top) { StatusBanner() }
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    CloseNowPlaying()
                }
                if track != nil {
                    ToolbarItem(placement: .topBarTrailing) {
                        SharePlayingSong()
                    }
                }
                ToolbarItem(placement: .topBarTrailing) {
                    PlayerMoreMenu(songPages: track.map { !$0.isServed } ?? false, asks: asks)
                }
            }
            .sheet(item: $asks.picked) { PlaylistPicker(tracks: $0.tracks).sheetSized() }
            .alert("player.queue.save", isPresented: $asks.savingQueue) {
                PlaylistNameField(confirm: said("common.save")) { library.features.createPlaylist($0, with: library.queue.filter { !$0.isServed }) }
            }
            .confirmationDialog("player.queue.clear.question", isPresented: $asks.clearingQueue, titleVisibility: .visible) {
                Button("player.queue.clear", role: .destructive) { library.clearQueue() }
            } message: {
                Text("player.queue.clear.message")
            }
            .modifier(DeleteSongsConfirmation(insidePlayer: true))
            .modifier(PlaylistPickerHost(insidePlayer: true))
            .modifier(SharingSheet(insidePlayer: true))
            .modifier(SleepTimerPresentation(insidePlayer: true))
            .modifier(ZonesPresentation(insidePlayer: true))
            .modifier(PairingOffer(insidePlayer: true))
            .modifier(PartyOffer(insidePlayer: true))
        }
    }

    @ViewBuilder
    private func arranged(_ track: Track?, _ arrangement: PlayerArrangement) -> some View {
        switch arrangement.seam {
        case let .beside(leading, gap, trailing):
            atTheFold(track, scrolls: arrangement.scrolls, leading: leading, gap: gap, trailing: trailing)
        case let .stacked(top, gap, bottom):
            onATable(track, scrolls: arrangement.scrolls, top: top, gap: gap, bottom: bottom)
        case .none:
            if arrangement.sideBySide {
                columns(track, scrolls: arrangement.scrolls)
            } else if arrangement.scrolls {
                ScrollView { layout(track) }
            } else {
                layout(track)
            }
        }
    }

    private func columns(_ track: Track?, scrolls: Bool) -> some View {
        HStack(alignment: .center, spacing: Self.gutter) {
            leadingColumn(track, scrolls: scrolls)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            trailingColumn(track, scrolls: scrolls)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        .padding(.bottom, 8)
    }

    private func atTheFold(_ track: Track?, scrolls: Bool, leading: Double, gap: Double, trailing: Double) -> some View {
        HStack(alignment: .center, spacing: 0) {
            leadingColumn(track, scrolls: scrolls)
                .frame(width: leading)
                .frame(maxHeight: .infinity)
            Color.clear.frame(width: gap)
            trailingColumn(track, scrolls: scrolls, bleeding: .trailing)
                .frame(width: trailing)
                .frame(maxHeight: .infinity)
        }
        .padding(.bottom, 8)
    }

    private func onATable(_ track: Track?, scrolls: Bool, top: Double, gap: Double, bottom: Double) -> some View {
        VStack(spacing: 0) {
            Group {
                if mode == .artwork {
                    LargeArtwork()
                } else {
                    focusedPane(track)
                }
            }
            .frame(maxWidth: .infinity)
            .frame(height: top)
            Color.clear.frame(height: gap)
            controls(track, scrolls: scrolls)
                .frame(height: bottom)
        }
    }

    @ViewBuilder
    private func leadingColumn(_ track: Track?, scrolls: Bool) -> some View {
        if mode == .artwork {
            LargeArtwork()
        } else {
            controls(track, scrolls: scrolls)
        }
    }

    @ViewBuilder
    private func trailingColumn(_ track: Track?, scrolls: Bool, bleeding: Edge.Set = .horizontal) -> some View {
        if mode == .artwork {
            controls(track, scrolls: scrolls)
        } else {
            focusedPane(track, bleeding: bleeding)
        }
    }

    @ViewBuilder
    private func controls(_ track: Track?, scrolls: Bool) -> some View {
        if mode == .artwork {
            column(scrolls: scrolls) {
                PlayerTitles(track: track)
                PlayerScrubber()
                TransportRow()
                SleepCountdown()
                VolumeRow()
                PlayerModes(mode: mode, choose: choose)
            }
        } else {
            column(scrolls: scrolls) {
                CompactTitles(track: track)
                PlayerScrubber()
                TransportRow()
                PlayerModes(mode: mode, choose: choose)
            }
        }
    }

    private func focusedPane(_ track: Track?, bleeding: Edge.Set = .horizontal) -> some View {
        ZStack {
            if settled == mode {
                focusedContent(track, bleeding: bleeding).transition(.opacity)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    @ViewBuilder
    private func column<Content: View>(scrolls: Bool, @ViewBuilder content: () -> Content) -> some View {
        if scrolls {
            ScrollView {
                VStack(spacing: 16) { content() }
            }
            .frame(maxWidth: .infinity)
        } else {
            VStack(spacing: 16) {
                Spacer(minLength: 0)
                content()
                Spacer(minLength: 0)
            }
            .frame(maxWidth: .infinity)
        }
    }

    private func layout(_ track: Track?) -> some View {
        let large = mode == .artwork
        let focusedHeight: CGFloat? = typeSize.isAccessibilitySize ? Self.focusedHeightAtAccessibilitySizes : nil
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
                .frame(maxWidth: .infinity, minHeight: focusedHeight, maxHeight: focusedHeight ?? .infinity)
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
    private func focusedContent(_ track: Track?, bleeding: Edge.Set = .horizontal) -> some View {
        switch mode {
        case .lyrics: LyricsPane(track: track, compact: true)
        case .queue, .artwork: UpNextList().padding(bleeding, -Self.gutter)
        }
    }

    private func open() async {
        FrameRateBoost.shared.boost(for: Self.openedWithin)
        try? await Task.sleep(for: .seconds(Self.openedWithin))
        rest.opening = false
    }

    private func choose(_ target: PlayerMode) {
        guard !reduceMotion else {
            mode = target
            withAnimation(Self.arriving) { settled = target == .artwork ? nil : target }
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
        let rest = rest
        rest.reshaping = true
        withAnimation(Self.reshaping, completionCriteria: .removed) {
            mode = target
        } completion: {
            rest.reshaping = false
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
    private static let swipeNoticedPast: CGFloat = 24

    @Environment(Navigation.self) private var navigation

    var body: some View {
        Color.clear
            .aspectRatio(1, contentMode: .fit)
            .anchorPreference(key: CoverSlot.self, value: .bounds) { [.large: $0] }
            .padding(.horizontal, 8)
            .frame(maxWidth: .infinity)
            .padding(.horizontal, -NowPlayingView.gutter)
            .contentShape(.rect)
            .simultaneousGesture(swipeDown)
            .padding(.horizontal, NowPlayingView.gutter)
    }

    private var swipeDown: some Gesture {
        DragGesture(minimumDistance: Self.swipeNoticedPast)
            .onEnded { drag in
                guard CoverSwipe.closes(
                    across: drag.translation.width,
                    down: drag.translation.height,
                    flungDown: drag.predictedEndTranslation.height
                ) else { return }
                navigation.nowPlaying = false
            }
    }
}

struct MovingCover: View {
    private static let pausedScale: CGFloat = 0.84
    private static let corner: CGFloat = 24
    private static let restsOver: Double = 0.5
    private static let settlesOver: Double = 0.25
    private static let settlesOnItsSlot: Animation = .smooth(duration: settlesOver)
    private static let boostOutlasts: CFTimeInterval = 0.1
    private static let restedWithin: CFTimeInterval = restsOver + boostOutlasts
    private static let handedOverWithin: CFTimeInterval = MotionSpan.handsOver + settlesOver + boostOutlasts

    @Environment(Library.self) private var library
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @Environment(\.displayScale) private var displayScale
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
            .motion(.spring(duration: Self.restsOver, bounce: 0.25), value: playing)
            .scaleEffect(slot.width / drawn)
            .offset(x: slot.midX - drawn / 2, y: slot.midY - drawn / 2)
            .transaction(value: slot) { moved in
                if moved.animation == nil, !reduceMotion { moved.animation = Self.settlesOnItsSlot }
            }
            .handover(value: track?.pictureKey)
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            .onChange(of: slot, initial: true) { _, now in
                guard large, now.width > 0, now.width != edge else { return }
                edge = now.width
                NowPlayingOpening.coverMeasured(ArtworkBucket.fitting(points: now.width, scale: displayScale))
            }
            .onChange(of: playing) {
                if large { boost(for: Self.restedWithin) }
            }
            .onChange(of: track?.pictureKey) {
                boost(for: Self.handedOverWithin)
            }
    }

    private func boost(for seconds: CFTimeInterval) {
        guard !reduceMotion, UIApplication.shared.applicationState == .active else { return }
        FrameRateBoost.shared.boost(for: seconds)
    }
}

@MainActor
enum NowPlayingOpening {
    private static var coverBucket: ArtworkBucket?

    static func coverMeasured(_ bucket: ArtworkBucket) {
        coverBucket = bucket
    }

    static func prepare(_ track: Track, features: Features) async {
        await BackdropView.prepare(track, features: features)
        guard let bucket = coverBucket, !track.isServed, let cover = await features.artwork(track, bucket: bucket) else { return }
        ShownArtwork.keep(cover, for: track)
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
        Canvas(rendersAsynchronously: true) { context, size in
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
    @State private var tallest = HeldHeight()

    var body: some View {
        HStack(alignment: .center, spacing: 12) {
            words
                .contentTransition(.opacity)
                .handover(value: track?.pictureKey)
                .frame(maxWidth: .infinity, alignment: .leading)
            if let track { FavouriteButton(track: track).font(.title2) }
        }
        .onGeometryChange(for: CGSize.self) { $0.size } action: { size in
            var seen = tallest
            seen.see(width: size.width, height: size.height)
            if seen != tallest { tallest = seen }
        }
        .frame(minHeight: tallest.height, alignment: .bottom)
    }

    private var words: some View {
        VStack(alignment: .leading, spacing: 4) {
            if let track {
                SongMenu(track: track) {
                    Text(track.shownTitle)
                        .font(.title2.weight(.bold))
                        .foregroundStyle(Color.primary)
                        .titleLines(2)
                        .multilineTextAlignment(.leading)
                }
                SongMenu(track: track) {
                    Text(track.shownArtist)
                        .font(.title3)
                        .nowPlayingInk()
                        .titleLines()
                }
                if !track.album.isEmpty {
                    SongMenu(track: track) {
                        Text(track.album)
                            .font(.subheadline)
                            .nowPlayingInk()
                            .titleLines()
                    }
                }
            } else {
                Text("player.nothing-playing")
                    .font(.title2.weight(.bold))
                    .titleLines(2)
            }
        }
        .lineLimit(1)
    }
}

struct SongMenu<Label: View>: View {
    let track: Track
    @ViewBuilder let label: () -> Label

    var body: some View {
        if SongDestinations.any(for: track) {
            Menu {
                SongDestinations(track: track)
            } label: {
                label()
            }
            .menuStyle(.button)
            .buttonStyle(.plain)
            .tint(Color.primary)
            .accessibilityHint("player.song-menu.hint")
        } else {
            label()
        }
    }
}

struct SongDestinations: View {
    @Environment(Navigation.self) private var navigation
    let track: Track

    static func any(for track: Track) -> Bool {
        !track.isServed && (track.albumPage != nil || !track.artist.isEmpty)
    }

    var body: some View {
        if let albumPage = track.albumPage {
            Button("player.go-to-album", systemImage: "square.stack") { navigation.open(albumPage) }
        }
        if !track.artist.isEmpty {
            Button("player.go-to-artist", systemImage: "music.mic") { navigation.open(.artist(track.artist)) }
        }
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
                .handover(value: track?.pictureKey)
            Spacer(minLength: 0)
            if let track { FavouriteButton(track: track).font(.title3) }
        }
    }

    @ViewBuilder
    private var words: some View {
        if let track {
            SongMenu(track: track) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(track.shownTitle).font(.headline).foregroundStyle(Color.primary).titleLines()
                    Text(track.shownArtist).font(.subheadline).nowPlayingInk().titleLines()
                }
                .accessibilityElement(children: .combine)
            }
        } else {
            Text("player.nothing-playing").font(.headline).titleLines()
        }
    }
}

struct PlayerScrubber: View {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @Environment(\.accessibilityReduceTransparency) private var reduceTransparency
    @Environment(\.colorSchemeContrast) private var contrast
    @Environment(\.colorScheme) private var scheme
    @Environment(\.dynamicTypeSize) private var typeSize
    private static let nearEnoughMs: Double = 1500
    private static let holdAtMost: Duration = .seconds(4)
    private static let restingHeight: CGFloat = 7
    private static let grabbedHeight: CGFloat = 14
    private static let touchHeight: CGFloat = 44
    private static let stepMs: Double = 10_000
    private static let slightestMove: CGFloat = 2
    private static let grabSettles: CFTimeInterval = 0.4

    @Environment(Library.self) private var library
    @State private var dragging: Double?
    @State private var grabbedAt: Double?
    @State private var held: Double?
    @State private var grabs = 0
    @State private var course = ScrubberCourse(to: 0, pace: .snap)
    @State private var lastReading = LastReading()
    @State private var heading = SliderDrag.undecided
    @GestureState private var touching = false

    private final class LastReading {
        var at: CFTimeInterval = 0
        var song: String?
    }

    var body: some View {
        let timing = library.clock
        let duration = Double(max(timing.durationMs, 1))
        let live = Double(timing.positionMs)
        let shown = min(dragging ?? held ?? live, duration)
        let drawn = min(dragging ?? held ?? course.to, duration)
        let grabbed = dragging != nil

        VStack(spacing: 6) {
            GeometryReader { space in
                let width = max(space.size.width, 1)
                Capsule()
                    .fill(.primary.opacity(0.16))
                    .overlay {
                        ScrubberFill(
                            fraction: drawn / duration,
                            course: course,
                            ink: NowPlayingInk.shown(grabbed ? 0.95 : 0.7, contrast: contrast, reduceTransparency: reduceTransparency),
                            dark: scheme == .dark,
                            turnsInk: !reduceMotion
                        )
                        .accessibilityHidden(true)
                    }
                    .clipShape(Capsule())
                    .trackOutline()
                    .frame(height: grabbed ? Self.grabbedHeight : Self.restingHeight)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .overlay {
                        Color.clear
                            .frame(height: Self.touchHeight)
                            .contentShape(Rectangle())
                            .simultaneousGesture(drag(across: width, of: duration, from: shown))
                    }
            }
            .frame(height: Self.grabbedHeight)
            .motion(.spring(duration: 0.3, bounce: 0.3), value: grabbed)
            .accessibilityElement()
            .accessibilityLabel("player.position")
            .accessibilityValue(said("player.position.value", clock(UInt64(shown)), clock(timing.durationMs)))
            .accessibilityAdjustableAction { direction in
                let step = direction == .increment ? Self.stepMs : -Self.stepMs
                commit(min(max(shown + step, 0), duration))
            }
            .background { seekKeys(from: shown, of: duration) }
            VStack(spacing: 4) {
                HStack {
                    Text(clock(UInt64(shown)))
                        .accessibilityHidden(true)
                    Spacer()
                    if !typeSize.isAccessibilitySize { outputLine }
                    Spacer()
                    Text("-" + clock(UInt64(max(duration - shown, 0))))
                        .accessibilityHidden(true)
                }
                if typeSize.isAccessibilitySize { outputLine }
            }
            .font(.caption.monospacedDigit())
            .nowPlayingInk(grabbed ? 1 : NowPlayingInk.secondary)
            .motion(.smooth, value: grabbed)
        }
        .allowsHitTesting(library.loaded)
        .sensoryFeedback(.impact(weight: .light), trigger: grabs)
        .onChange(of: touching) { _, now in
            guard !now else { return }
            heading = .undecided
            grabbedAt = nil
            dragging = nil
        }
        .onChange(of: timing.positionMs, initial: true) { old, now in
            if let target = held, abs(Double(now) - target) < Self.nearEnoughMs { held = nil }
            flow(from: Double(old), to: Double(now))
        }
        .onChange(of: grabbed) {
            if !reduceMotion { FrameRateBoost.shared.boost(for: Self.grabSettles) }
        }
        .onChange(of: library.playingTrack?.pictureKey) {
            held = nil
            course = ScrubberCourse(to: Double(library.clock.positionMs), pace: .snap)
        }
        .onChange(of: library.transport) {
            let settles = MotionRendering.of(.flip, reduceMotion: reduceMotion, sceneActive: true) == .animated
            course = ScrubberCourse(to: held ?? Double(library.clock.positionMs), pace: settles ? .settle(over: MotionSpan.flipsOver) : .snap)
        }
        .task(id: held) {
            guard held != nil else { return }
            try? await Task.sleep(for: Self.holdAtMost)
            if !Task.isCancelled { held = nil }
        }
    }

    @ViewBuilder
    private var outputLine: some View {
        if let waiting = library.waitingForADevice {
            WaitingLine(text: waiting)
        } else {
            QualityLine(track: library.playingTrack)
        }
    }

    private func seekKeys(from shown: Double, of duration: Double) -> some View {
        HStack {
            Button("player.seek.back") { commit(min(max(shown - Self.stepMs, 0), duration)) }
                .keyboardShortcut(.leftArrow, modifiers: [])
            Button("player.seek.forward") { commit(min(max(shown + Self.stepMs, 0), duration)) }
                .keyboardShortcut(.rightArrow, modifiers: [])
        }
        .opacity(0)
        .allowsHitTesting(false)
        .accessibilityHidden(true)
    }

    private func drag(across width: CGFloat, of duration: Double, from shown: Double) -> some Gesture {
        DragGesture(minimumDistance: 0)
            .updating($touching) { _, held, _ in held = true }
            .onChanged { change in
                heading = heading.after(across: change.translation.width, down: change.translation.height)
                guard heading != .declined else {
                    if dragging != nil { dragging = nil }
                    return
                }

                let start = grabbedAt ?? shown
                if grabbedAt == nil {
                    grabbedAt = start
                    grabs += 1
                }
                let moved = Double(change.translation.width / width) * duration
                dragging = min(max(start + moved, 0), duration)
            }
            .onEnded { change in
                let slid = heading != .declined
                heading = .undecided
                grabbedAt = nil
                let target = dragging
                dragging = nil
                guard slid, let target, abs(change.translation.width) >= Self.slightestMove else { return }
                commit(target)
            }
    }

    private func commit(_ target: Double) {
        held = target
        course = ScrubberCourse(to: target, pace: .snap)
        library.seek(to: UInt64(target))
    }

    private func flow(from old: Double, to now: Double) {
        let at = CACurrentMediaTime()
        let song = library.playingTrack?.pictureKey
        let reading = ProgressFlow.Reading(old: old, now: now, sinceLast: lastReading.at == 0 ? nil : at - lastReading.at)
        let conditions = ProgressFlow.Conditions(
            playing: library.transport == .playing,
            touched: dragging != nil,
            held: held,
            reduceMotion: reduceMotion,
            songChanged: SongChange(from: lastReading.song, to: song).handsOver
        )
        lastReading.at = at
        lastReading.song = song
        switch ProgressFlow.step(reading, conditions) {
        case .snap(let target):
            course = ScrubberCourse(to: target, pace: .snap)
        case .flow(let target, let over):
            course = ScrubberCourse(to: target, pace: .flow(over: over))
        }
    }
}

struct WaitingLine: View {
    let text: String

    var body: some View {
        Label(text, systemImage: "speaker.slash")
            .font(.caption2.weight(.semibold))
            .nowPlayingInk()
            .titleLines(2)
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
                .titleLines()
                .multilineTextAlignment(.center)
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
        if let bits = track.bits { parts.append(said("player.quality.bits", Int(bits))) }
        if track.rate > 0 { parts.append(kilohertz(track.rate)) }
        if output > 0 { parts.append(said("player.quality.output", kilohertz(output))) }
        return parts.filter { !$0.isEmpty }.joined(separator: " · ")
    }

    private static func kilohertz(_ rate: UInt32) -> String {
        said("player.quality.rate", String(format: "%g", Double(rate) / 1000))
    }
}

private struct TrackOutline: ViewModifier {
    @Environment(\.colorSchemeContrast) private var contrast

    func body(content: Content) -> some View {
        content.overlay {
            if contrast == .increased {
                Capsule().strokeBorder(Color.primary, lineWidth: 1)
            }
        }
    }
}

extension View {
    fileprivate func trackOutline() -> some View {
        modifier(TrackOutline())
    }
}

struct NowPlayingInk: ViewModifier {
    static let secondary = 0.95
    static let dimmed = 0.7

    @Environment(\.colorSchemeContrast) private var contrast
    @Environment(\.accessibilityReduceTransparency) private var reduceTransparency
    let level: Double

    static func shown(_ level: Double, contrast: ColorSchemeContrast, reduceTransparency: Bool) -> Double {
        contrast == .increased || reduceTransparency ? 1 : level
    }

    func body(content: Content) -> some View {
        content.foregroundStyle(Color.primary.opacity(Self.shown(level, contrast: contrast, reduceTransparency: reduceTransparency)))
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

private struct ModeInk: ViewModifier {
    @Environment(\.colorSchemeContrast) private var contrast
    let active: Bool

    func body(content: Content) -> some View {
        content.foregroundStyle(active ? Color.worn : Color.primary.opacity(contrast == .increased ? 1 : NowPlayingInk.dimmed))
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

    fileprivate func modeInk(_ active: Bool) -> some View {
        modifier(ModeInk(active: active))
    }
}

struct TransportRow: View {
    @Environment(Library.self) private var library
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var skipped = 0

    var body: some View {
        let shuffled = library.shuffled
        let repeating = library.repeatMode
        GlassEffectContainer(spacing: 24) {
            HStack(spacing: 0) {
                Button { library.setShuffle(!shuffled) } label: { Label("player.shuffle", systemImage: "shuffle").touchTarget().buttonShape() }
                    .modeInk(shuffled)
                    .modeMark(shuffled)
                    .motion(Flip.animation, value: shuffled)
                    .accessibilityAddTraits(.isToggle)
                    .accessibilityValue(said(shuffled ? "common.on" : "common.off"))
                    .accessibilityInputLabels([said("player.shuffle"), said("player.shuffle.input.songs")])
                    .frame(maxWidth: .infinity)
                Button {
                    skipped += 1
                    library.previous()
                } label: {
                    Label("player.previous", systemImage: "backward.fill").touchTarget().buttonShape()
                }
                .font(.title)
                .accessibilityInputLabels([said("player.previous"), said("player.previous.input.song"), said("player.previous.input.back")])
                .frame(maxWidth: .infinity)
                PlayButton(transport: library.transport, large: true)
                Button {
                    skipped += 1
                    library.next()
                } label: {
                    Label("player.next", systemImage: "forward.fill").touchTarget().buttonShape()
                }
                .font(.title)
                .accessibilityInputLabels([said("player.next"), said("player.next.input.song"), said("player.next.input.skip")])
                .frame(maxWidth: .infinity)
                Button { library.setRepeat(repeating.following) } label: {
                    Label("player.repeat", systemImage: repeating.symbol)
                        .contentTransition(reduceMotion ? .identity : .symbolEffect(.replace))
                        .touchTarget()
                        .buttonShape()
                }
                    .modeInk(repeating != .off)
                    .modeMark(repeating != .off)
                    .motion(Flip.animation, value: repeating)
                    .accessibilityAddTraits(.isToggle)
                    .accessibilityValue(repeating.spoken)
                    .accessibilityInputLabels([said("player.repeat"), said("player.repeat.input.songs")])
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
        if let text = Self.text(library.sleepTimer, remaining: library.clock.sleepRemainingMs) {
            Button {
                navigation.sleepTimer = true
            } label: {
                Label(text, systemImage: "moon.zzz.fill")
                    .font(.footnote.monospacedDigit())
                    .nowPlayingInk()
            }
            .buttonStyle(.plain)
            .accessibilityHint("player.sleep.hint")
        }
    }

    static func text(_ timer: SleepTimer?, remaining: UInt64?) -> String? {
        switch timer {
        case .after: said("player.sleep.pausing-in", clock((remaining ?? 0) + 999))
        case .endOfTrack: said("player.sleep.end-of-song.pauses")
        case .endOfQueue: said("player.sleep.end-of-queue.pauses")
        case nil: nil
        }
    }
}

struct VolumeRow: View {
    private static let restingHeight: CGFloat = 7
    private static let grabbedHeight: CGFloat = 14
    private static let touchHeight: CGFloat = 44
    private static let step: Float = 1 / 16
    private static let driverMountsAfter: Duration = .milliseconds(600)
    private static let stepFollowedOver: Double = 0.25
    private static let stepFollowed: Animation = .smooth(duration: stepFollowedOver)
    private static let grabSettles: CFTimeInterval = 0.4

    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var volume = SystemVolumeLevel()
    @State private var dragging: Float?
    @State private var grabbedAt: Float?
    @State private var driven = false
    @State private var heading = SliderDrag.undecided
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
                            .nowPlayingInk(grabbed ? 0.95 : 0.7)
                            .frame(width: width * CGFloat(shown))
                            .motion(
                                volume.steppedOutside && !grabbed ? Self.stepFollowed : nil,
                                value: shown
                            )
                    }
                    .clipShape(Capsule())
                    .trackOutline()
                    .frame(height: grabbed ? Self.grabbedHeight : Self.restingHeight)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .contentShape(Rectangle())
                    .simultaneousGesture(drag(across: width, from: shown))
            }
            .frame(height: Self.touchHeight)
            .background {
                if driven { VolumeDriver(volume: volume).allowsHitTesting(false) }
            }
            .motion(.spring(duration: 0.3, bounce: 0.3), value: grabbed)
            Image(systemName: "speaker.wave.3.fill").nowPlayingInk().accessibilityHidden(true)
        }
        .accessibilityElement()
        .accessibilityLabel("player.volume")
        .accessibilityValue(Text(Double(shown), format: .percent.precision(.fractionLength(0))))
        .accessibilityAdjustableAction { direction in
            let step = direction == .increment ? Self.step : -Self.step
            driven = true
            volume.set(min(max(shown + step, 0), 1))
        }
        .onChange(of: touching) { _, now in
            guard !now else { return }
            heading = .undecided
            grabbedAt = nil
            dragging = nil
        }
        .onChange(of: grabbed) {
            if !reduceMotion { FrameRateBoost.shared.boost(for: Self.grabSettles) }
        }
        .onChange(of: volume.level) {
            if volume.steppedOutside, !reduceMotion { FrameRateBoost.shared.boost(for: Self.stepFollowedOver) }
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
                heading = heading.after(across: change.translation.width, down: change.translation.height)
                guard heading != .declined else {
                    if dragging != nil { dragging = nil }
                    return
                }

                let start = grabbedAt ?? shown
                if grabbedAt == nil { grabbedAt = start }
                driven = true
                let moved = Float(change.translation.width / width)
                let wanted = min(max(start + moved, 0), 1)

                dragging = wanted
                volume.set(wanted)
            }
            .onEnded { _ in
                heading = .undecided
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
    private(set) var steppedOutside = false
    @ObservationIgnored weak var driver: MPVolumeView?
    @ObservationIgnored private weak var foundSlider: UISlider?
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
                    Self.hideDrawing(of: slider)
                    foundSlider = slider
                    if watchAsked, !heardSinceWatching, !sessionAnswered { show(slider.value) }
                    return
                }
                try? await Task.sleep(for: Self.seedEvery)
            }
        }
    }

    private func hear(_ heard: Float) {
        heardSinceWatching = true
        show(heard, steppedOutside: true)
    }

    private func show(_ shown: Float, steppedOutside outside: Bool = false) {
        if steppedOutside != outside { steppedOutside = outside }
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
        guard let slider = driverSlider else { return }
        slider.setValue(wanted, animated: false)
        slider.sendActions(for: .valueChanged)
    }

    private var driverSlider: UISlider? {
        guard let driver else { return nil }
        if let foundSlider, foundSlider.isDescendant(of: driver) { return foundSlider }
        guard let slider = Self.slider(in: driver) else { return nil }
        Self.hideDrawing(of: slider)
        foundSlider = slider
        return slider
    }

    nonisolated private static func observing(
        _ heard: @escaping @MainActor @Sendable (Float) -> Void
    ) -> NSKeyValueObservation {
        AVAudioSession.sharedInstance().observe(\.outputVolume, options: [.new]) { _, change in
            guard let value = change.newValue else { return }
            Task { @MainActor in heard(value) }
        }
    }

    private static func hideDrawing(of slider: UISlider) {
        guard slider.currentThumbImage?.size != .zero else { return }
        let nothing = UIImage()
        for state in [UIControl.State.normal, .highlighted, .selected, .disabled] {
            slider.setThumbImage(nothing, for: state)
        }
        slider.minimumTrackTintColor = .clear
        slider.maximumTrackTintColor = .clear
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
    let volume: SystemVolumeLevel

    func makeUIView(context: Context) -> KeptControlHost {
        let driver = KeptControls.volume.control
        volume.driver = driver
        volume.seed(from: driver)
        return KeptControls.volume.host()
    }

    func updateUIView(_ uiView: KeptControlHost, context: Context) {
        volume.driver = KeptControls.volume.control
    }
}

struct PlayerModes: View {
    @ScaledMetric(relativeTo: .title3) private var edge: CGFloat = 48
    @Environment(\.accessibilityDifferentiateWithoutColor) private var differentiate
    let mode: PlayerMode
    let choose: (PlayerMode) -> Void
    @State private var switched = 0

    var body: some View {
        GlassEffectContainer(spacing: 20) {
            HStack(spacing: 20) {
                toggle(.lyrics, title: said("player.mode.lyrics"), symbol: "quote.bubble")
                RoutePicker()
                    .frame(width: edge, height: edge)
                    .glassEffect(.regular.interactive(), in: .circle)
                    .accessibilityLabel("AirPlay")
                toggle(.queue, title: said("player.mode.up-next"), symbol: "list.bullet")
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
                .frame(width: edge, height: edge)
                .contentShape(.circle)
        }
        .foregroundStyle(active ? Color.wornInk : Color.primary)
        .glassEffect(active ? .regular.tint(Color.worn).interactive() : .regular.interactive(), in: .circle)
        .buttonStyle(.plain)
        .modeMark(active && differentiate)
        .accessibilityAddTraits(.isToggle)
        .accessibilityValue(said(active ? "common.on" : "common.off"))
        .accessibilityInputLabels([title, said("player.mode.show", title)])
    }
}

@MainActor
@Observable
final class PlayerAsks {
    var picked: PickedTracks?
    var savingQueue = false
    var clearingQueue = false
}

@MainActor
@Observable
final class FieldRest {
    var opening = true
    var reshaping = false
}

struct NowPlayingField: View {
    let track: Track?
    let rest: FieldRest

    var body: some View {
        BackdropView(track: track, resting: rest.opening || rest.reshaping)
    }
}

struct CloseNowPlaying: View {
    @Environment(Navigation.self) private var navigation

    var body: some View {
        Button("common.close", systemImage: "chevron.down") { navigation.nowPlaying = false }
    }
}

struct PlayerMoreMenu: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    let songPages: Bool
    let asks: PlayerAsks

    var body: some View {
        Menu("player.more", systemImage: "ellipsis") {
            if songPages {
                Section {
                    Button("player.add-to-playlist", systemImage: "text.badge.plus") {
                        playing { asks.picked = PickedTracks(tracks: [$0]) }
                    }
                    SharePlayingSong()
                    Button("player.song-details", systemImage: "info.circle") { playing { navigation.open(.inspector($0)) } }
                    Button("player.analyse", systemImage: "waveform.path.ecg") { playing { navigation.open(.analysis($0)) } }
                }
            }
            if library.queueShape != .empty {
                Section("player.queue") {
                    Button("player.queue.save", systemImage: "text.badge.plus") { asks.savingQueue = true }
                    Button("player.queue.clear", systemImage: "trash", role: .destructive) { asks.clearingQueue = true }
                        .disabled(library.queueShape == .one)
                }
            }
            Section {
                Button("player.sleep-timer", systemImage: library.sleepTimer == nil ? "moon.zzz" : "moon.zzz.fill") {
                    navigation.sleepTimer = true
                }
                Button("player.zones", systemImage: "hifispeaker.2") { navigation.zones = true }
                Button("player.equaliser", systemImage: "slider.vertical.3") {
                    navigation.tab = .settings
                    navigation.open(.sound)
                }
                Button("player.visualizer", systemImage: "waveform") { navigation.open(.visualizer) }
                Button("player.output", systemImage: "hifispeaker") {
                    navigation.tab = .settings
                    navigation.open(.output)
                }
            }
        }
    }

    private func playing(_ act: (Track) -> Void) {
        guard let track = library.playingTrack else { return }
        act(track)
    }
}

@MainActor
enum PlayerWarmup {
    private static let afterLaunch: Duration = .seconds(2)

    static func warm() async {
        try? await Task.sleep(for: afterLaunch)
        guard !Task.isCancelled else { return }
        KeptControls.warm()
    }
}

struct RoutePicker: UIViewRepresentable {
    var whileRouted = Color.worn

    func makeUIView(context: Context) -> KeptControlHost {
        KeptControls.route.host()
    }

    func updateUIView(_ uiView: KeptControlHost, context: Context) {
        KeptControls.route.control.activeTintColor = UIColor(whileRouted)
    }
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
            .onChange(of: library.queueShape == .empty) {
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
                Text("player.queue.empty").secondaryInk()
            } else if let playing {
                let parts = QueueParts(
                    rows: rows.count,
                    playing: playing,
                    next: library.playingNext,
                    heardAtMost: heard,
                    aheadAtMost: ahead
                )
                let placing = NextPlacement(playing: playing, next: parts.next)
                if playing > 0 {
                    Section {
                        QueueHeading(title: said("player.queue.history"))
                        if parts.history.lowerBound > 0 {
                            moreRows(said("player.queue.earlier", parts.history.lowerBound)) {
                                heard += Self.widenedBy
                            }
                        }
                        rowsOf(rows, parts.history, placing: placing).opacity(Self.heardFaded)
                    }
                }
                Section {
                    QueueHeading(title: said("player.queue.now-playing")).id(playingRowId)
                    QueueRow(row: rows[playing], current: true, edited: $edited)
                }
                if parts.next.isEmpty {
                    Section {
                        QueueHeading(title: said("player.queue.up-next"))
                        if parts.rest.isEmpty, parts.beyond == 0 {
                            Text("player.queue.nothing-more").secondaryInk()
                        } else {
                            rowsOf(rows, parts.rest, placing: placing)
                        }
                        if parts.beyond > 0 {
                            further(parts.beyond)
                        }
                    }
                } else {
                    Section {
                        QueueHeading(title: said("player.queue.playing-next"))
                        joinedRows(rows, parts, placing: placing)
                        if parts.rest.isEmpty, parts.beyond == 0 {
                            Text("player.queue.nothing-more").secondaryInk()
                        }
                        if parts.beyond > 0 {
                            further(parts.beyond)
                        }
                    }
                }
            } else {
                let shown = min(rows.count, ahead)
                Section {
                    QueueHeading(title: said("player.queue.up-next"))
                    rowsOf(rows, 0..<shown, placing: nil)
                    if rows.count > shown {
                        further(rows.count - shown)
                    }
                }
            }
        }
        .sensoryFeedback(.selection, trigger: edited)
    }

    private func further(_ count: Int) -> some View {
        moreRows(said("player.queue.more", count)) { ahead += Self.widenedBy }
    }

    private func moreRows(_ title: String, _ widen: @escaping () -> Void) -> some View {
        Button(action: widen) {
            HStack {
                Text(title)
                Spacer()
                Text("player.queue.show").font(.footnote.weight(.semibold))
            }
            .secondaryInk()
            .contentShape(.rect)
        }
        .buttonStyle(.plain)
        .listRowBackground(Color.clear)
        .accessibilityHint("player.queue.show.hint")
    }

    private func rowsOf(_ rows: [Track], _ range: Range<Int>, placing: NextPlacement?) -> some View {
        ForEach(range.map { QueueEntry.row(rows[$0], at: $0) }) { entry in
            queueRow(entry, placing: placing)
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

    private func joinedRows(_ rows: [Track], _ parts: QueueParts, placing: NextPlacement) -> some View {
        let joint = parts.next.count
        let lower = parts.next.lowerBound
        let entries = parts.next.map { QueueEntry.row(rows[$0], at: $0) } + [QueueEntry.joint] + parts.rest.map { QueueEntry.row(rows[$0], at: $0) }

        return ForEach(entries) { entry in
            switch entry {
            case .row:
                queueRow(entry, placing: placing)
            case .joint:
                QueueHeading(title: said("player.queue.continue-playing"))
                    .moveDisabled(true)
                    .deleteDisabled(true)
            }
        }
        .onMove { offsets, destination in
            guard let moved = offsets.first,
                let move = RowMoves.joined(offset: moved, destination: destination, joint: joint, within: lower)
            else { return }
            edited += 1
            library.moveInQueue(from: move.from, to: move.to)
        }
        .onDelete { offsets in
            edited += 1
            for offset in offsets.sorted(by: >) where offset != joint {
                library.removeFromQueue(rows[lower + (offset < joint ? offset : offset - 1)])
            }
        }
    }

    @ViewBuilder
    private func queueRow(_ entry: QueueEntry, placing: NextPlacement?) -> some View {
        if case .row(let row, let index) = entry {
            QueueRow(
                row: row,
                current: false,
                edited: $edited,
                movesToNext: placing?.takes(index).map { QueueMove(from: index, to: $0) }
            )
        }
    }
}

enum QueueEntry: Identifiable {
    enum ID: Hashable {
        case row(UInt64?)
        case joint
    }

    case row(Track, at: Int)
    case joint

    var id: ID {
        switch self {
        case .row(let track, _): .row(track.queueId)
        case .joint: .joint
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

struct QueueMove: Equatable {
    let from: Int
    let to: Int
}

struct QueueRow: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    let row: Track
    let current: Bool
    @Binding var edited: Int
    var movesToNext: QueueMove?

    var body: some View {
        let favourite = library.isFavourite(row)
        TrackRow(track: row, current: current, ownsMenus: false) {
            library.jump(to: row)
        }
        .swipeActions(edge: .trailing) {
            Button("player.queue.remove", systemImage: "minus.circle", role: .destructive) {
                edited += 1
                library.removeFromQueue(row)
            }
            .tint(.red)
        }
        .contextMenu {
            if let movesToNext {
                Section {
                    Button("player.queue.move-to-next", systemImage: "text.line.first.and.arrowtriangle.forward") {
                        edited += 1
                        library.moveInQueue(from: movesToNext.from, to: movesToNext.to)
                    }
                }
            }
            if !row.isServed {
                Section {
                    Button(said(favourite ? "favourite.remove" : "favourite.add"), systemImage: favourite ? "heart.slash" : "heart") {
                        library.setFavourite(row, !favourite)
                    }
                    Button("player.add-to-playlist", systemImage: "text.badge.plus") { navigation.addToPlaylist([row]) }
                }
                Section {
                    SongPages(track: row)
                }
            }
            Section {
                Button("player.queue.remove", systemImage: "minus.circle", role: .destructive) {
                    edited += 1
                    library.removeFromQueue(row)
                }
            }
        }
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
                    said(handoff.playing ? "player.handoff.playing" : "player.handoff", Int(handoff.songs), handoff.device),
                    systemImage: "arrow.triangle.2.circlepath"
                )
            }
        }
    }
}

struct SleepTimerPresentation: ViewModifier {
    @Environment(Navigation.self) private var navigation
    let insidePlayer: Bool

    func body(content: Content) -> some View {
        content.sheet(isPresented: Binding(
            get: { navigation.nowPlaying == insidePlayer && navigation.sleepTimer },
            set: { navigation.sleepTimer = $0 }
        )) {
            SleepTimerSheet().sheetSized().presentationDetents([.medium, .large])
        }
    }
}

struct SleepTimerSheet: View {
    private static let minutes: [UInt32] = [5, 15, 30, 45, 60, 90]

    @Environment(Library.self) private var library
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        let timer = library.sleepTimer
        NavigationStack {
            List {
                Section {
                    SleepTimerState()
                }
                Section {
                    ForEach(Self.minutes, id: \.self) { minutes in
                        Button(said("counted.minutes", Int(minutes))) { set(.after(seconds: minutes * 60)) }
                    }
                    Button("player.sleep.end-of-song") { set(.endOfTrack) }
                    Button("player.sleep.end-of-queue") { set(.endOfQueue) }
                }
                if timer != nil {
                    Button("player.sleep.cancel", role: .destructive) { set(nil) }
                }
            }
            .navigationTitle("player.sleep-timer")
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
        if let text = SleepCountdown.text(library.sleepTimer, remaining: library.clock.sleepRemainingMs) {
            Text(text).font(.headline.monospacedDigit())
        } else {
            Text("player.sleep.none").foregroundStyle(.secondary)
        }
    }
}
