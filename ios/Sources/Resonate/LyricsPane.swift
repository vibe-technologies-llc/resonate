import ResonateNative
import SwiftUI
import UIKit

struct LyricsPane: View {
    @Environment(Library.self) private var library
    @State private var stage: LyricStage
    @State private var visible = false
    @State private var lookedLong = false

    let track: Track?
    let compact: Bool

    init(track: Track?, compact: Bool = false) {
        self.track = track
        self.compact = compact
        _stage = State(initialValue: LyricStage())
    }

    var body: some View {
        let features = library.features
        let sheet = features.lyrics?.trackId == track?.id ? features.lyrics : nil
        Group {
            if track == nil {
                ContentUnavailableView(
                    "Nothing playing",
                    systemImage: "quote.bubble",
                    description: Text("Play a song to follow its words.")
                )
            } else if let sheet, !sheet.lines.isEmpty {
                LyricFollowing(sheet: sheet, track: track, stage: stage, visible: visible, compact: compact)
            } else if features.lyricsLoading || sheet == nil {
                looking
            } else {
                ContentUnavailableView(
                    "No lyrics",
                    systemImage: "quote.bubble",
                    description: Text(
                        features.online
                            ? "No lyrics found for this song"
                            : "No saved lyrics. Enable online features to look them up, or add an LRC beside the song."
                    )
                )
            }
        }
        .task(id: track?.id) {
            if let track { features.loadLyrics(track.id) }
        }
        .task(id: features.lyricsLoading) {
            lookedLong = false
            guard features.lyricsLoading else { return }
            try? await Task.sleep(for: LyricPace.looksQuietlyFor)
            if !Task.isCancelled { lookedLong = true }
        }
        .onAppear { visible = true }
        .onDisappear {
            visible = false
            stage.rest()
        }
    }

    @ViewBuilder private var looking: some View {
        if lookedLong {
            ProgressView { Text("Looking for lyrics…") }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        } else {
            Color.clear
        }
    }
}

private struct LyricFollowing: View {
    @Environment(Library.self) private var library
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @Environment(\.colorScheme) private var scheme
    @Environment(\.colorSchemeContrast) private var contrast
    @Environment(\.accessibilityReduceTransparency) private var reduceTransparency
    @Environment(\.accessibilityVoiceOverEnabled) private var voiceOver
    @Environment(\.displayScale) private var displayScale
    @Environment(\.scenePhase) private var phase
    @Environment(\.self) private var environment
    @ScaledMetric(relativeTo: .title) private var fullWords: CGFloat = 32
    @ScaledMetric(relativeTo: .title2) private var compactWords: CGFloat = 28

    let sheet: LyricSheet
    let track: Track?
    let stage: LyricStage
    let visible: Bool
    let compact: Bool

    var body: some View {
        let playback = LyricPlayback.of(library.snapshot, at: library.clock.positionMs, showing: track)
        let measures = LyricMeasures.at(words: compact ? compactWords : fullWords, compact: compact)
        let inks = LyricInks(
            scheme: scheme,
            accent: Color.worn.resolve(in: environment),
            legibility: contrast == .increased || reduceTransparency ? .solid : .glass
        )
        let awake = visible && phase == .active
        let sounding = playback?.playing == true && sheet.detail != .unsynced
        let turning = awake && stage.isMoving(at: Date.now.timeIntervalSinceReferenceDate)
        let animating = turning || (awake && sounding && sheet.detail == .words)
        let ticking = !animating && awake && sounding
        let native = library.native
        let still = reduceMotion
        let scale = displayScale
        let compact = compact
        _ = stage.beat

        return GeometryReader { proxy in
            let content = { (date: Date, running: Bool) -> LyricSheetView in
                let now = (running ? date : Date.now).timeIntervalSinceReferenceDate
                let scene = stage.scene(
                    of: LyricCue(
                        sheet: sheet,
                        playback: playback,
                        native: native,
                        pane: proxy.size,
                        now: now,
                        measures: measures,
                        inks: inks,
                        still: still,
                        scale: scale,
                        awake: awake,
                        animating: animating,
                        culls: !voiceOver
                    )
                )
                return LyricSheetView(
                    scene: scene,
                    stage: stage,
                    measures: measures,
                    pane: proxy.size,
                    compact: compact,
                    seek: seek
                )
            }
            TimelineView(LyricSchedule(ticking: ticking, paused: !animating)) { timeline in
                content(timeline.date, ticking || animating)
            }
            .fullFrameRate(while: animating)
        }
    }

    private func seek(to moment: UInt64) {
        stage.followAgain(at: Date.now.timeIntervalSinceReferenceDate)
        library.request { try $0.seek(positionMs: moment) }
    }
}

struct LyricSchedule: TimelineSchedule {
    let ticking: Bool
    let paused: Bool

    func entries(from start: Date, mode: TimelineScheduleMode) -> AnySequence<Date> {
        if ticking {
            return AnySequence(PeriodicTimelineSchedule(from: start, by: LyricPace.lineSyncedTick).entries(from: start, mode: mode))
        }
        return AnySequence(AnimationTimelineSchedule(minimumInterval: nil, paused: paused).entries(from: start, mode: mode))
    }
}

struct LyricAttribution: View {
    let sheet: LyricSheet

    var body: some View {
        HStack(spacing: 8) {
            Text(timing)
                .font(.caption2.weight(.semibold).monospaced())
                .tracking(0.6)
                .padding(.horizontal, 7)
                .padding(.vertical, 3)
                .overlay(Capsule().strokeBorder(.secondary.opacity(0.6), lineWidth: 1))
            if !sheet.origin.isEmpty {
                Text(sheet.origin).font(.caption2.monospaced())
            }
        }
        .foregroundStyle(.secondary)
        .padding(.vertical, 8)
        .accessibilityElement(children: .combine)
        .accessibilityLabel(sheet.origin.isEmpty ? spokenTiming : "\(spokenTiming), from \(sheet.origin)")
    }

    private var timing: String {
        switch sheet.detail {
        case .words: "WORD-SYNCED"
        case .lines: "SYNCED"
        case .unsynced: "UNSYNCED"
        }
    }

    private var spokenTiming: String {
        switch sheet.detail {
        case .words: "Lyrics timed word by word"
        case .lines: "Lyrics timed line by line"
        case .unsynced: "Lyrics without timing"
        }
    }
}

private struct LyricSheetView: View {
    @GestureState private var held = false

    let scene: LyricScene
    let stage: LyricStage
    let measures: LyricMeasures
    let pane: CGSize
    let compact: Bool
    let seek: (UInt64) -> Void

    var body: some View {
        ZStack(alignment: .bottom) {
            sheet
            follow
        }
        .frame(width: pane.width, height: pane.height)
        .contentShape(.rect)
        .simultaneousGesture(hand)
        .onChange(of: held) { _, holding in
            guard !holding else { return }
            Task { stage.letGo(at: Date.now.timeIntervalSinceReferenceDate) }
        }
        .accessibilityElement(children: .contain)
        .accessibilityScrollAction { edge in
            stage.turnPage(towards: edge, at: Date.now.timeIntervalSinceReferenceDate)
        }
    }

    private var sheet: some View {
        let generation = scene.generation
        return VStack(spacing: measures.spacing) {
            ForEach(LyricPiece.gathered(scene.rows, spacing: measures.spacing)) { piece in
                switch piece {
                case .row(let row):
                    LyricRowView(look: row.look, measures: measures)
                        .equatable()
                        .onGeometryChange(for: CGRect.self) { proxy in
                            proxy.frame(in: .named(LyricSpace.sheet))
                        } action: { frame in
                            stage.measured(row: row.id, frame, generation: generation)
                        }
                        .offset(y: row.inset)
                        .onTapGesture {
                            if let moment = row.moment { seek(moment) }
                        }
                        .allowsHitTesting(row.moment != nil)
                        .modifier(LyricRowAccessibility(row: row, seek: seek))
                case .gap(_, let width, let height):
                    Color.clear
                        .frame(width: width, height: height)
                        .accessibilityHidden(true)
                }
            }
            if let end = scene.end {
                LyricEndMark(look: end.look, measures: measures)
                    .equatable()
                    .onGeometryChange(for: CGRect.self) { proxy in
                        proxy.frame(in: .named(LyricSpace.sheet))
                    } action: { frame in
                        stage.measured(row: end.id, frame, generation: generation)
                    }
                    .offset(y: end.inset)
            }
        }
        .padding(.top, scene.head)
        .padding(.bottom, scene.tail)
        .frame(width: pane.width)
        .fixedSize(horizontal: false, vertical: true)
        .coordinateSpace(.named(LyricSpace.sheet))
        .onGeometryChange(for: CGFloat.self) { proxy in
            proxy.size.height
        } action: { height in
            stage.measuredSheet(height, generation: generation)
        }
        .id(generation)
        .offset(y: scene.offset)
        .frame(width: pane.width, height: pane.height, alignment: .top)
        .mask(dissolving)
        .clipped()
        .contentShape(.rect)
        .opacity(scene.shown)
    }

    private var dissolving: some View {
        let edge = pane.height > 0 ? min(measures.dissolve / pane.height, 0.25) : 0
        return LinearGradient(
            stops: [
                .init(color: .clear, location: 0),
                .init(color: .black, location: edge),
                .init(color: .black, location: 1 - edge),
                .init(color: .clear, location: 1),
            ],
            startPoint: .top,
            endPoint: .bottom
        )
    }

    private var follow: some View {
        Button {
            stage.followAgain(at: Date.now.timeIntervalSinceReferenceDate)
        } label: {
            Label("Follow", systemImage: "quote.bubble")
        }
        .buttonStyle(.glass)
        .controlSize(compact ? .small : .regular)
        .padding(.bottom, compact ? 6 : 12)
        .opacity(scene.offersFollow ? 1 : 0)
        .allowsHitTesting(scene.offersFollow)
        .accessibilityHidden(!scene.offersFollow)
        .accessibilityHint("Scrolls back to the words being sung")
        .animation(.easeInOut(duration: LyricPace.turn), value: scene.offersFollow)
    }

    private var hand: some Gesture {
        DragGesture(minimumDistance: 10)
            .updating($held) { _, holding, _ in holding = true }
            .onChanged { value in
                stage.dragged(by: value.translation, at: Date.now.timeIntervalSinceReferenceDate)
            }
            .onEnded { value in
                stage.released(velocity: value.velocity.height, at: Date.now.timeIntervalSinceReferenceDate)
            }
    }
}

private struct LyricRowAccessibility: ViewModifier {
    let row: LyricSceneRow
    let seek: (UInt64) -> Void

    func body(content: Content) -> some View {
        if row.look.blank {
            content.accessibilityHidden(true)
        } else if let moment = row.moment {
            content
                .accessibilityElement(children: .ignore)
                .accessibilityLabel(row.look.text)
                .accessibilityValue(clock(moment))
                .accessibilityAddTraits(row.lit ? [.isButton, .isSelected] : .isButton)
                .accessibilityAction { seek(moment) }
        } else {
            content
                .accessibilityElement(children: .ignore)
                .accessibilityLabel(row.look.text)
                .accessibilityAddTraits(row.lit ? .isSelected : [])
        }
    }
}

private struct LyricRowView: View, Equatable {
    let look: LyricRowLook
    let measures: LyricMeasures

    var body: some View {
        if look.blank {
            Color.clear.frame(width: look.width, height: measures.pause)
        } else {
            VStack(spacing: 0) {
                if look.breathes {
                    ZStack {
                        if let breath = look.breath {
                            LyricBreather(breath: breath, measures: measures)
                        }
                    }
                    .padding(.horizontal, measures.pad)
                    .frame(width: look.width, height: measures.breath, alignment: look.side.alignment)
                }
                words
                    .font(.system(size: measures.words, weight: .bold))
                    .lineSpacing(measures.lineSpacing)
                    .multilineTextAlignment(look.side.textAlignment)
                    .fixedSize(horizontal: false, vertical: true)
                    .frame(width: look.textWidth, alignment: look.side.alignment)
                    .padding(.horizontal, measures.pad)
                    .padding(.vertical, measures.padding)
                    .frame(width: look.width, alignment: look.side.alignment)
                    .opacity(look.standing)
            }
        }
    }

    @ViewBuilder private var words: some View {
        switch look.words {
        case .plain(let text, let ink): Text(text).foregroundStyle(Color(ink))
        case .swept(let text): Text(text)
        }
    }
}

private struct LyricBreather: View {
    let breath: LyricBreathLook
    let measures: LyricMeasures

    var body: some View {
        let widest = measures.dot * (1 + LyricPace.dotSwellsBy)
        HStack(spacing: measures.dotGap) {
            ForEach(0..<LyricPace.breathDots, id: \.self) { dot in
                let lit = min(max(breath.through * Double(LyricPace.breathDots) - Double(dot), 0), 1)
                let side = measures.dot * (1 + LyricPace.dotSwellsBy * breath.swell * (0.5 + 0.5 * lit))
                Circle()
                    .fill(Color(breath.ink).opacity(LyricPace.dimDot + (1 - LyricPace.dimDot) * lit))
                    .frame(width: side, height: side)
                    .frame(width: widest, height: widest)
            }
        }
        .opacity(breath.opacity)
        .accessibilityHidden(true)
    }
}

private struct LyricEndMark: View, Equatable {
    let look: LyricEndLook
    let measures: LyricMeasures

    var body: some View {
        HStack(spacing: measures.endGap) {
            rule
            Text("END OF LYRICS")
                .font(.caption.weight(.semibold))
                .tracking(1.4)
                .foregroundStyle(Color(look.muted))
            rule
        }
        .padding(.vertical, measures.endPadding)
        .opacity(look.standing)
        .frame(width: look.width)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("End of lyrics")
    }

    private var rule: some View {
        Rectangle()
            .fill(Color(look.faint))
            .frame(width: measures.endRule, height: 1)
    }
}

enum LyricSpace {
    static let sheet = "lyric-sheet"
}

enum LyricPace {
    static let lineSyncedTick: TimeInterval = 0.05
    static let cullsBeyondPanes = 1.0
    static let cullsWithin: CGFloat = 320

    static let turn: TimeInterval = 0.42
    static let glide: TimeInterval = 0.6
    static let launchedAtMost = 4.0
    static let lagPerLine: TimeInterval = 0.014
    static let lagsAtMost = 6
    static let arrivesIn: TimeInterval = 0.56
    static let rise: TimeInterval = 0.6
    static let risePerLine: TimeInterval = 0.04
    static let risesAtMost = 8
    static let breath: TimeInterval = 2.4
    static let driftsAtMost: TimeInterval = 0.25
    static let steeredOver: TimeInterval = 0.55
    static let handsOff: TimeInterval = 6
    static let coastsFor: TimeInterval = 0.5
    static let settled = 0.5
    static let lit = 1.0
    static let adrift = 0.8
    static let unsungShare = 0.38
    static let solidUnsungShare = 0.7
    static let solidStandingFloor = 0.85
    static let voiceOfTwoSpans = 0.84
    static let dimDot = 0.22
    static let dotSwellsBy = 0.23
    static let breathDots = 3
    static let bandedOver = 0.55
    static let pageTurned = 0.8
    static let looksQuietlyFor: Duration = .milliseconds(450)

    static func share(_ elapsed: TimeInterval, of span: TimeInterval) -> Double {
        min(max(elapsed / span, 0), 1)
    }

    static func easedInOut(_ through: Double) -> Double {
        if through < 0.5 { return 2 * through * through }
        let left = -2 * through + 2
        return 1 - left * left / 2
    }

    static func landed(_ through: Double) -> Double {
        let left = 1 - through
        return 1 - (3 * through + 1) * left * left * left
    }

    static func landingPace(_ through: Double) -> Double {
        12 * through * (1 - through) * (1 - through)
    }

    static func launched(_ through: Double) -> Double {
        let left = 1 - through
        return through * left * left * left
    }

    static func launchPace(_ through: Double) -> Double {
        let left = 1 - through
        return left * left * (1 - 4 * through)
    }

    static func lagged(_ lines: Int, by perLine: TimeInterval, atMost: Int) -> TimeInterval {
        perLine * Double(min(max(lines, 0), atMost))
    }
}

enum LyricFalloff: Equatable {
    case around
    case across

    func between(sung: Int, line: Int) -> Double {
        line < sung ? behind(sung - line) : ahead(line - sung)
    }

    func ahead(_ away: Int) -> Double {
        switch (self, away) {
        case (_, 0): LyricPace.lit
        case (.around, 1): 0.4
        case (.around, 2): 0.18
        case (.around, _): 0
        case (.across, 1): 0.42
        case (.across, 2): 0.3
        case (.across, 3): 0.22
        case (.across, _): 0.16
        }
    }

    func behind(_ away: Int) -> Double {
        switch self {
        case .around: 0
        case .across: ahead(away)
        }
    }

    func spent(read: Int?, line: Int) -> Double {
        switch (self, read) {
        case (.around, let read?) where line >= read: ahead(line - read + 1)
        case (.around, _): 0
        case (.across, let read?): ahead(max(abs(read - line), 1))
        case (.across, nil): ahead(.max)
        }
    }
}

enum LyricReading: Equatable {
    case at(Int)
    case evenly
    case spent(Int?)

    init(_ reads: LyricReads) {
        switch reads {
        case .at(let line): self = .at(Int(line))
        case .spent(let line): self = .spent(line.map(Int.init))
        case .evenly: self = .evenly
        }
    }
}

struct LyricVoicesLit: Equatable {
    static let none = LyricVoicesLit(one: nil, two: nil)

    let one: Int?
    let two: Int?

    func lights(_ line: Int) -> Bool {
        one == line || two == line
    }
}

struct LyricTurn<Value: Equatable> {
    private(set) var from: Value
    private(set) var to: Value
    private var started: TimeInterval

    init(still value: Value) {
        from = value
        to = value
        started = -.infinity
    }

    mutating func onto(_ value: Value, at now: TimeInterval) {
        guard value != to else { return }
        from = to
        to = value
        started = now
    }

    func blended(at now: TimeInterval, _ reading: (Value) -> Double) -> Double {
        let through = LyricPace.easedInOut(LyricPace.share(now - started, of: LyricPace.turn))
        return reading(from) * (1 - through) + reading(to) * through
    }

    func settled(at now: TimeInterval) -> Bool {
        now - started >= LyricPace.turn
    }
}

struct LyricLeg {
    let was: Double
    let lands: Double
    let launch: Double
    let started: TimeInterval

    static func launched(from was: Double, to lands: Double, pace: Double, at now: TimeInterval) -> LyricLeg {
        let reach = abs(lands - was) * LyricPace.launchedAtMost
        return LyricLeg(was: was, lands: lands, launch: min(max(pace, -reach), reach), started: now)
    }

    func elapsed(at now: TimeInterval) -> TimeInterval {
        max(now - started, 0)
    }

    func after(_ elapsed: TimeInterval) -> Double {
        let through = LyricPace.share(elapsed, of: LyricPace.glide)
        return was + (lands - was) * LyricPace.landed(through) + launch * LyricPace.launched(through)
    }

    func pace(after elapsed: TimeInterval) -> Double {
        let through = LyricPace.share(elapsed, of: LyricPace.glide)
        return (lands - was) * LyricPace.landingPace(through) + launch * LyricPace.launchPace(through)
    }

    func landed(at now: TimeInterval) -> Bool {
        elapsed(at: now) >= LyricPace.glide
    }
}

struct LyricGlide {
    let leg: LyricLeg
    let before: LyricLeg?

    static func fromRest(was: Double, lands: Double, at now: TimeInterval) -> LyricGlide {
        LyricGlide(leg: LyricLeg(was: was, lands: lands, launch: 0, started: now), before: nil)
    }

    func at(_ now: TimeInterval) -> Double {
        leg.after(leg.elapsed(at: now))
    }

    func lagging(_ lag: TimeInterval, at now: TimeInterval) -> Double {
        let into = leg.elapsed(at: now)
        if let before, into < lag {
            return before.after(max(before.elapsed(at: now) - lag, 0))
        }
        return leg.after(max(into - lag, 0))
    }

    func redirected(to lands: Double, at now: TimeInterval) -> LyricGlide {
        let into = leg.elapsed(at: now)
        let was = leg.after(into)
        return LyricGlide(leg: .launched(from: was, to: lands, pace: leg.pace(after: into), at: now), before: leg)
    }

    func settled(at now: TimeInterval) -> Bool {
        leg.elapsed(at: now) >= LyricPace.glide + LyricPace.lagged(LyricPace.lagsAtMost, by: LyricPace.lagPerLine, atMost: LyricPace.lagsAtMost)
    }
}

struct LyricPlayback: Equatable {
    let track: UInt64
    let positionMs: UInt64
    let playing: Bool

    static func of(_ snapshot: Snapshot?, at positionMs: UInt64, showing track: Track?) -> LyricPlayback? {
        guard let snapshot, let current = snapshot.current, let track, current.id == track.id else { return nil }
        return LyricPlayback(track: current.id, positionMs: positionMs, playing: snapshot.transport == .playing)
    }
}

struct LyricClock {
    private var heard: LyricPlayback?
    private var heardAt: TimeInterval = 0
    private var track: UInt64?
    private var from: TimeInterval = 0
    private var at: TimeInterval = 0
    private var running = false

    mutating func kept(_ playback: LyricPlayback, at now: TimeInterval) -> TimeInterval {
        if heard != playback {
            heard = playback
            heardAt = now
        }
        let sounding = playback.playing ? max(now - heardAt, 0) : 0
        let sampled = Double(playback.positionMs) / 1000 + sounding
        let elapsed = max(now - at, 0)
        let ran = from + elapsed

        guard running, track == playback.track, playback.playing, abs(ran - sampled) <= LyricPace.driftsAtMost else {
            track = playback.track
            from = sampled
            at = now
            running = playback.playing
            return sampled
        }

        let share = 1 - exp(-elapsed / LyricPace.steeredOver)
        let steered = max(ran + (sampled - ran) * share, from)
        from = steered
        at = now
        return steered
    }

    mutating func stop() {
        self = LyricClock()
    }
}

struct LyricAsked: Equatable {
    let track: UInt64
    let positionMs: UInt64
}

struct LyricAsk: Sendable {
    let native: MobilePlayer
    let track: UInt64
    let positionMs: UInt64
}

final class LyricFrameSource: @unchecked Sendable {
    private static let queue = DispatchQueue(label: "org.resonate.lyric-frames", qos: .userInteractive)

    private let lock = NSLock()
    private var asking = false
    private var owed: LyricAsk?
    private let landed: @Sendable (LyricFrame, UInt64) -> Void

    init(landed: @escaping @Sendable (LyricFrame, UInt64) -> Void) {
        self.landed = landed
    }

    func ask(_ ask: LyricAsk) {
        let starting = lock.withLock {
            if asking {
                owed = ask
                return false
            }
            asking = true
            return true
        }
        if starting { answer(ask) }
    }

    private func answer(_ ask: LyricAsk) {
        Self.queue.async { [self] in
            landed(ask.native.lyricFrame(id: ask.track, positionMs: ask.positionMs), ask.track)
            let next = lock.withLock {
                let next = owed
                owed = nil
                asking = next != nil
                return next
            }
            if let next { answer(next) }
        }
    }
}

struct LyricMeasures: Equatable {
    let words: CGFloat
    let lineSpacing: CGFloat
    let padding: CGFloat
    let spacing: CGFloat
    let breath: CGFloat
    let pause: CGFloat
    let margin: CGFloat
    let column: CGFloat
    let gutter: CGFloat
    let pad: CGFloat
    let dot: CGFloat
    let dotGap: CGFloat
    let endRule: CGFloat
    let endGap: CGFloat
    let endPadding: CGFloat
    let dissolve: CGFloat
    let rise: CGFloat

    private static let largestWords: CGFloat = 56
    private static let leading: CGFloat = 1.28

    static func at(words asked: CGFloat, compact: Bool) -> LyricMeasures {
        let words = min(asked, largestWords)
        let natural = UIFont.systemFont(ofSize: words, weight: .bold).lineHeight
        return LyricMeasures(
            words: words,
            lineSpacing: max(words * leading - natural, 0),
            padding: 8,
            spacing: 4,
            breath: 28,
            pause: 18,
            margin: compact ? 24 : 48,
            column: 720,
            gutter: compact ? 8 : 16,
            pad: 12,
            dot: 7,
            dotGap: 10,
            endRule: 36,
            endGap: 12,
            endPadding: 16,
            dissolve: compact ? 40 : 72,
            rise: 22
        )
    }
}

enum LyricLegibility: Equatable {
    case glass
    case solid
}

struct LyricInks: Equatable {
    let lit: Color.Resolved
    let second: Color.Resolved
    let muted: Color.Resolved
    let faint: Color.Resolved
    let accent: Color.Resolved
    let unsungShare: Double
    let standingFloor: Double

    private static let mutedShare: Float = 0.5
    private static let faintShare: Float = 0.22
    private static let solidFaintShare: Float = 0.6

    init(scheme: ColorScheme, accent: Color.Resolved, legibility: LyricLegibility) {
        let lit = scheme == .dark
            ? Color.Resolved(red: 1, green: 1, blue: 1, opacity: 1)
            : Color.Resolved(red: 0.06, green: 0.06, blue: 0.08, opacity: 0.92)
        self.lit = lit
        second = accent
        let solid = legibility == .solid
        muted = solid ? lit : Self.thinned(lit, to: Self.mutedShare)
        faint = Self.thinned(lit, to: solid ? Self.solidFaintShare : Self.faintShare)
        self.accent = accent
        unsungShare = solid ? LyricPace.solidUnsungShare : LyricPace.unsungShare
        standingFloor = solid ? LyricPace.solidStandingFloor : 0
    }

    static func thinned(_ ink: Color.Resolved, to share: Float) -> Color.Resolved {
        var thinned = ink
        thinned.opacity = ink.opacity * share
        return thinned
    }

    static func mixed(_ from: Color.Resolved, _ to: Color.Resolved, by share: Double) -> Color.Resolved {
        let toward = Float(min(max(share, 0), 1))
        let away = 1 - toward
        return Color.Resolved(
            red: from.red * away + to.red * toward,
            green: from.green * away + to.green * toward,
            blue: from.blue * away + to.blue * toward,
            opacity: from.opacity * away + to.opacity * toward
        )
    }
}

enum LyricSide: Equatable {
    case centre
    case leading
    case trailing

    var alignment: Alignment {
        switch self {
        case .centre: .center
        case .leading: .leading
        case .trailing: .trailing
        }
    }

    var textAlignment: TextAlignment {
        switch self {
        case .centre: .center
        case .leading: .leading
        case .trailing: .trailing
        }
    }
}

enum LyricWords: Equatable {
    case plain(String, Color.Resolved)
    case swept(AttributedString)

    static func swept(_ text: String, by wipe: LyricWipe, unsung: Color.Resolved, sung: Color.Resolved) -> LyricWords {
        let units = text.utf16.count
        let sungTo = min(Int(wipe.sungUtf16), units)
        let blendFrom = min(max(Int(wipe.blend?.startUtf16 ?? wipe.sungUtf16), sungTo), units)
        let blendTo = min(max(Int(wipe.blend?.endUtf16 ?? wipe.sungUtf16), blendFrom), units)
        let blending = LyricInks.mixed(unsung, sung, by: Double(wipe.blend?.share ?? 0))
        let pieces = cut(text, atUTF16: [sungTo, blendFrom, blendTo])

        var whole = AttributedString()
        for (piece, ink) in zip(pieces, [sung, unsung, blending, unsung]) where !piece.isEmpty {
            var part = AttributedString(piece)
            part.foregroundColor = Color(ink)
            whole.append(part)
        }
        return .swept(whole)
    }

    static func cut(_ text: String, atUTF16 cuts: [Int]) -> [String] {
        var pieces: [String] = []
        var piece = String.UnicodeScalarView()
        var units = 0
        var next = 0

        for scalar in text.unicodeScalars {
            while next < cuts.count, units >= cuts[next] {
                pieces.append(String(piece))
                piece = String.UnicodeScalarView()
                next += 1
            }
            piece.append(scalar)
            units += scalar.utf16.count
        }
        while next < cuts.count {
            pieces.append(String(piece))
            piece = String.UnicodeScalarView()
            next += 1
        }
        pieces.append(String(piece))
        return pieces
    }
}

struct LyricBreathLook: Equatable {
    let through: Double
    let swell: Double
    let opacity: Double
    let ink: Color.Resolved
}

struct LyricRowLook: Equatable {
    let culled: CGFloat?
    let blank: Bool
    let text: String
    let words: LyricWords
    let side: LyricSide
    let width: CGFloat
    let textWidth: CGFloat
    let standing: Double
    let breathes: Bool
    let breath: LyricBreathLook?
}

enum LyricPiece: Identifiable {
    case row(LyricSceneRow)
    case gap(first: Int, width: CGFloat, height: CGFloat)

    var id: Int {
        switch self {
        case .row(let row): row.id
        case .gap(let first, _, _): -1 - first
        }
    }

    static func gathered(_ rows: [LyricSceneRow], spacing: CGFloat) -> [LyricPiece] {
        var pieces: [LyricPiece] = []
        var open: (first: Int, width: CGFloat, height: CGFloat)?

        for row in rows {
            if let height = row.look.culled {
                open = open.map { ($0.first, $0.width, $0.height + spacing + height) } ?? (row.id, row.look.width, height)
                continue
            }
            if let gap = open {
                pieces.append(.gap(first: gap.first, width: gap.width, height: gap.height))
                open = nil
            }
            pieces.append(.row(row))
        }
        if let gap = open {
            pieces.append(.gap(first: gap.first, width: gap.width, height: gap.height))
        }
        return pieces
    }
}

struct LyricSceneRow: Identifiable {
    let id: Int
    let look: LyricRowLook
    let inset: CGFloat
    let moment: UInt64?
    let lit: Bool
}

struct LyricEndLook: Equatable {
    let standing: Double
    let width: CGFloat
    let muted: Color.Resolved
    let faint: Color.Resolved
}

struct LyricSceneEnd {
    let id: Int
    let look: LyricEndLook
    let inset: CGFloat
}

struct LyricScene {
    let generation: Int
    let rows: [LyricSceneRow]
    let end: LyricSceneEnd?
    let offset: CGFloat
    let shown: Double
    let head: CGFloat
    let tail: CGFloat
    let offersFollow: Bool
}

struct LyricCue {
    let sheet: LyricSheet
    let playback: LyricPlayback?
    let native: MobilePlayer?
    let pane: CGSize
    let now: TimeInterval
    let measures: LyricMeasures
    let inks: LyricInks
    let still: Bool
    let scale: CGFloat
    let awake: Bool
    let animating: Bool
    let culls: Bool
}

struct LyricColumn: Equatable {
    let width: CGFloat
    let measures: LyricMeasures
}

struct LyricBreathing {
    let line: Int
    let started: TimeInterval

    func opacity(at now: TimeInterval) -> Double {
        LyricPace.easedInOut(LyricPace.share(now - started, of: LyricPace.turn))
    }

    func fading(at now: TimeInterval) -> Double {
        1 - LyricPace.share(now - started, of: LyricPace.turn)
    }

    func settled(at now: TimeInterval) -> Bool {
        now - started >= LyricPace.turn
    }
}

struct LyricBreath {
    let through: Double
    let opacity: Double
}

struct LyricHand {
    var at: TimeInterval?
    var grabbedFrom: Double?
    var passedOver = false
    var coast: LyricLeg?
}

struct LyricLandmarks: Equatable {
    let readAt: UInt32?
    let reads: LyricReads
    let waitingFor: UInt32?
    let ended: Bool
    let voiceOne: UInt32?
    let voiceTwo: UInt32?

    init(_ frame: LyricFrame) {
        readAt = frame.readAt
        reads = frame.reads
        waitingFor = frame.waiting?.next
        ended = frame.ended
        voiceOne = frame.voiceOne
        voiceTwo = frame.voiceTwo
    }
}

@MainActor
@Observable
final class LyricStage {
    var beat = 0

    @ObservationIgnored private var sheet: LyricSheet?
    @ObservationIgnored private var generation = 0
    @ObservationIgnored private var frame: LyricFrame?
    @ObservationIgnored private var wipes: [Int: LyricWipe] = [:]
    @ObservationIgnored private var frames: LyricFrameSource?
    @ObservationIgnored private var asked: LyricAsked?
    @ObservationIgnored private var clock = LyricClock()
    @ObservationIgnored private var playingHere = false
    @ObservationIgnored private var canAsk = false
    @ObservationIgnored private var animating = false
    @ObservationIgnored private var rows: [Int: CGRect] = [:]
    @ObservationIgnored private var columns: [Int: LyricColumn] = [:]
    @ObservationIgnored private var sheetHeight: CGFloat = 0
    @ObservationIgnored private var pane: CGSize = .zero
    @ObservationIgnored private var measures = LyricMeasures.at(words: 32, compact: false)
    @ObservationIgnored private var scale: CGFloat = 1
    @ObservationIgnored private var placed = false
    @ObservationIgnored private var arrived: TimeInterval?
    @ObservationIgnored private var readAt: Int?
    @ObservationIgnored private var turn = LyricTurn(still: LyricReading.evenly)
    @ObservationIgnored private var light = LyricTurn(still: LyricVoicesLit.none)
    @ObservationIgnored private var spread = LyricTurn(still: LyricFalloff.around)
    @ObservationIgnored private var glide: LyricGlide?
    @ObservationIgnored private var offset: Double = 0
    @ObservationIgnored private var hand = LyricHand()
    @ObservationIgnored private var breathing: LyricBreathing?
    @ObservationIgnored private var fading: LyricBreathing?

    func scene(of cue: LyricCue) -> LyricScene {
        let now = cue.now

        hold(cue.sheet)
        pane = cue.pane
        measures = cue.measures
        scale = cue.scale
        animating = cue.animating
        playingHere = cue.playback != nil
        canAsk = cue.native != nil

        if let playback = cue.playback {
            let position = clock.kept(playback, at: now)
            if cue.awake, let native = cue.native {
                ask(native, track: playback.track, positionMs: UInt64(max(position, 0) * 1000))
            }
        } else {
            clock.stop()
        }

        letHandLapse(at: now)
        follow(at: now)
        place(at: now, still: cue.still)
        coast(at: now)

        let scene = drawn(at: now, cue: cue)

        if cue.awake, !cue.animating, isMoving(at: now) { nudge() }
        if cue.awake, cue.animating, synced, sheet?.detail == .lines, !isMoving(at: now) { nudge() }
        return scene
    }

    func isMoving(at now: TimeInterval) -> Bool {
        guard sheet != nil else { return false }
        return !placed && pane.height > 0
            || placed && unfollowed
            || !turn.settled(at: now)
            || !light.settled(at: now)
            || !spread.settled(at: now)
            || isArriving(at: now)
            || (hand.at != nil && synced && playingHere)
            || hand.grabbedFrom != nil
            || hand.coast != nil
            || isBreathingInOrOut(at: now)
            || glide.map { !$0.settled(at: now) } ?? false
    }

    func rest() {
        clock.stop()
        asked = nil
    }

    func land(_ landed: LyricFrame, for track: UInt64) {
        guard track == sheet?.trackId, playingHere else { return }
        if took(landed), !animating { beat &+= 1 }
    }

    private func hold(_ kept: LyricFrame, for track: UInt64) {
        guard track == sheet?.trackId, playingHere else { return }
        if took(kept), !animating { nudge() }
    }

    private func took(_ landed: LyricFrame) -> Bool {
        let moved = frame.map(LyricLandmarks.init) != LyricLandmarks(landed)

        frame = landed
        wipes = Dictionary(landed.wipes.map { (Int($0.line), $0) }) { kept, _ in kept }
        return moved
    }

    func measured(row: Int, _ frame: CGRect, generation: Int) {
        guard generation == self.generation else { return }
        rows[row] = frame
        columns[row] = LyricColumn(width: pane.width, measures: measures)
    }

    func measuredSheet(_ height: CGFloat, generation: Int) {
        guard generation == self.generation else { return }
        sheetHeight = height
    }

    func followAgain(at now: TimeInterval) {
        hand = LyricHand()
        spreadOut(at: now)
        beat &+= 1
    }

    func dragged(by translation: CGSize, at now: TimeInterval) {
        guard !hand.passedOver else { return }
        if hand.grabbedFrom == nil {
            guard placed, abs(translation.height) > abs(translation.width) else {
                hand.passedOver = true
                return
            }
            grab(at: now)
        }
        guard let from = hand.grabbedFrom else { return }
        offset = banded(from + translation.height)
        hand.at = now
    }

    func released(velocity: CGFloat, at now: TimeInterval) {
        hand.passedOver = false
        guard hand.grabbedFrom != nil else { return }
        hand.grabbedFrom = nil
        hand.at = now

        let bounds = scrollBounds
        let lands = min(max(offset + velocity * LyricPace.coastsFor, bounds.lowerBound), bounds.upperBound)
        hand.coast = .launched(from: offset, to: lands, pace: velocity * LyricPace.glide, at: now)
        beat &+= 1
    }

    func letGo(at now: TimeInterval) {
        hand.passedOver = false
        if hand.grabbedFrom != nil { released(velocity: 0, at: now) }
    }

    func turnPage(towards edge: Edge, at now: TimeInterval) {
        guard placed else { return }

        let page = Double(pane.height) * LyricPace.pageTurned
        let bounds = scrollBounds
        let turned: Double = switch edge {
        case .top: offset + page
        case .bottom: offset - page
        case .leading, .trailing: offset
        }

        grab(at: now)
        hand.grabbedFrom = nil
        offset = min(max(turned, bounds.lowerBound), bounds.upperBound)
    }

    private func grab(at now: TimeInterval) {
        hand.grabbedFrom = offset
        hand.coast = nil
        hand.at = now
        glide = nil
        spreadOut(at: now)
        beat &+= 1
    }

    private func nudge() {
        Task { @MainActor [weak self] in self?.beat &+= 1 }
    }

    private func ask(_ native: MobilePlayer, track: UInt64, positionMs: UInt64) {
        let asking = LyricAsked(track: track, positionMs: positionMs)
        guard asking != asked else { return }
        asked = asking

        if let kept = native.keptLyricFrame(id: track, positionMs: positionMs) {
            hold(kept, for: track)
            return
        }
        let source = frames ?? LyricFrameSource(landed: Self.delivering(to: self))
        frames = source
        source.ask(LyricAsk(native: native, track: track, positionMs: positionMs))
    }

    nonisolated private static func delivering(to stage: LyricStage) -> @Sendable (LyricFrame, UInt64) -> Void {
        { [weak stage] landed, track in
            DispatchQueue.main.async {
                MainActor.assumeIsolated { stage?.land(landed, for: track) }
            }
        }
    }

    private func hold(_ held: LyricSheet) {
        guard held != sheet else { return }
        sheet = held
        rewind()
    }

    private func rewind() {
        generation &+= 1
        frame = nil
        wipes = [:]
        asked = nil
        rows = [:]
        columns = [:]
        sheetHeight = 0
        placed = false
        arrived = nil
        readAt = nil
        turn = LyricTurn(still: .evenly)
        light = LyricTurn(still: .none)
        spread = LyricTurn(still: .around)
        glide = nil
        offset = 0
        hand = LyricHand()
        breathing = nil
        fading = nil
    }

    private var synced: Bool {
        sheet.map { $0.detail != .unsynced } ?? false
    }

    private func following(at now: TimeInterval) -> Bool {
        guard hand.grabbedFrom == nil else { return false }
        return hand.at.map { now - $0 >= LyricPace.handsOff } ?? true
    }

    private func letHandLapse(at now: TimeInterval) {
        guard hand.grabbedFrom == nil, let at = hand.at, now - at >= LyricPace.handsOff else { return }
        hand = LyricHand()
    }

    private func spreadOut(at now: TimeInterval) {
        spread.onto(following(at: now) ? .around : .across, at: now)
    }

    private var aimed: (reading: LyricReading, lit: LyricVoicesLit) {
        guard playingHere, let frame else { return (.evenly, .none) }
        return (
            LyricReading(frame.reads),
            LyricVoicesLit(one: frame.voiceOne.map(Int.init), two: frame.voiceTwo.map(Int.init))
        )
    }

    private var unfollowed: Bool {
        let aim = aimed
        return aim.reading != turn.to || aim.lit != light.to
    }

    private func follow(at now: TimeInterval) {
        spreadOut(at: now)
        guard playingHere, let frame else {
            readAt = nil
            breathing = nil
            fading = nil
            settle(on: .evenly, lit: .none, at: now)
            return
        }

        let lit = aimed.lit
        if let waiting = frame.waiting {
            let next = Int(waiting.next)
            if breathing?.line != next {
                breathing = LyricBreathing(line: next, started: now)
            }
            fading = nil
        } else {
            let breathedOut = breathing.flatMap { lit.lights($0.line) ? LyricBreathing(line: $0.line, started: now) : nil }
            fading = breathedOut ?? fading.flatMap { $0.settled(at: now) ? nil : $0 }
            breathing = nil
        }

        readAt = frame.readAt.map(Int.init)
        settle(on: LyricReading(frame.reads), lit: lit, at: now)
    }

    private func settle(on reading: LyricReading, lit: LyricVoicesLit, at now: TimeInterval) {
        if placed {
            turn.onto(reading, at: now)
            light.onto(lit, at: now)
        } else {
            turn = LyricTurn(still: reading)
            light = LyricTurn(still: lit)
        }
    }

    private var isSteady: Bool {
        guard let sheet, pane.height > 0, sheetHeight > 0 else { return false }
        let drawn = sheet.lines.count + (synced ? 1 : 0)
        let framed = frame != nil || !playingHere || !canAsk
        return framed && (0..<drawn).allSatisfy { rows[$0] != nil }
    }

    private func place(at now: TimeInterval, still: Bool) {
        if following(at: now), let lands = landing() {
            if placed {
                if still {
                    glide = nil
                    offset = lands
                } else {
                    glide(to: lands, at: now)
                }
            } else if isSteady {
                offset = lands
                glide = nil
                placed = true
                arrived = now
            }
        }
        if let glide { offset = glide.at(now) }
    }

    private func glide(to lands: Double, at now: TimeInterval) {
        switch glide {
        case nil:
            glide = .fromRest(was: offset, lands: lands, at: now)
        case let gliding? where abs(gliding.leg.lands - lands) < LyricPace.settled:
            return
        case let gliding?:
            glide = gliding.redirected(to: lands, at: now)
        }
    }

    private func coast(at now: TimeInterval) {
        guard hand.grabbedFrom == nil, let leg = hand.coast else { return }
        offset = leg.after(leg.elapsed(at: now))
        if leg.landed(at: now) { hand.coast = nil }
    }

    private func landing() -> Double? {
        if let line = readAt {
            let breathes = line < (sheet?.lines.count ?? 0) && sheet?.lines[line].breathes == true
            let room = breathes ? Double(measures.breath) : 0
            guard let centre = centre(of: line) else { return nil }
            return snapped(centre - room / 2)
        }
        if placed, !synced || !playingHere { return nil }
        return 0
    }

    private func centre(of index: Int) -> Double? {
        guard let row = rows[index], pane.height > 0 else { return nil }
        return Double((pane.height - row.height) / 2 - row.minY)
    }

    private var scrollBounds: ClosedRange<Double> {
        let lowest = min(Double(pane.height - sheetHeight), 0)
        return lowest...0
    }

    private func banded(_ wanted: Double) -> Double {
        let bounds = scrollBounds
        let span = max(Double(pane.height), 1)
        let past: Double
        if wanted > bounds.upperBound {
            past = wanted - bounds.upperBound
            return bounds.upperBound + stretched(past, over: span)
        }
        if wanted < bounds.lowerBound {
            past = bounds.lowerBound - wanted
            return bounds.lowerBound - stretched(past, over: span)
        }
        return wanted
    }

    private func stretched(_ past: Double, over span: Double) -> Double {
        (1 - 1 / (past * LyricPace.bandedOver / span + 1)) * span
    }

    private func snapped(_ length: Double) -> Double {
        (length * Double(scale)).rounded() / Double(scale)
    }

    private func ordinal(_ index: Int) -> Int {
        guard let sheet else { return index }
        if index < sheet.lines.count { return Int(sheet.lines[index].written) }
        if index == Int(sheet.end) { return Int(sheet.writtenEnd) }
        return index
    }

    private func standing(_ index: Int, at now: TimeInterval) -> Double {
        let line = ordinal(index)
        let focus = spread.blended(at: now) { falloff in
            turn.blended(at: now) { reading in
                switch reading {
                case .at(let sung): falloff.between(sung: ordinal(sung), line: line)
                case .evenly: LyricPace.adrift
                case .spent(let read): falloff.spent(read: read.map(ordinal), line: line)
                }
            }
        }
        return max(focus, lead(index, at: now))
    }

    private func lead(_ index: Int, at now: TimeInterval) -> Double {
        light.blended(at: now) { $0.lights(index) ? 1 : 0 }
    }

    private func inset(_ index: Int, at now: TimeInterval, still: Bool) -> CGFloat {
        let rise = rise(index, at: now, still: still)
        guard let glide else { return CGFloat(snapped(rise)) }
        let ahead = readAt.map { ordinal(index) - ordinal($0) } ?? 0
        let lag = LyricPace.lagged(ahead, by: LyricPace.lagPerLine, atMost: LyricPace.lagsAtMost)
        return CGFloat(snapped(glide.lagging(lag, at: now) + rise) - snapped(glide.at(now)))
    }

    private func rise(_ index: Int, at now: TimeInterval, still: Bool) -> Double {
        if still { return 0 }
        let risesFrom = Double(measures.rise)
        guard let arrived else { return risesFrom }
        let read = readAt.map(ordinal) ?? 0
        let lag = LyricPace.lagged(abs(ordinal(index) - read), by: LyricPace.risePerLine, atMost: LyricPace.risesAtMost)
        let risen = max(now - arrived - lag, 0)
        return risesFrom * (1 - LyricPace.landed(LyricPace.share(risen, of: LyricPace.rise)))
    }

    private func arrival(at now: TimeInterval) -> Double {
        guard placed, let arrived else { return 0 }
        return LyricPace.easedInOut(LyricPace.share(now - arrived, of: LyricPace.arrivesIn))
    }

    private func isArriving(at now: TimeInterval) -> Bool {
        guard let arrived else { return false }
        let lastRise = LyricPace.lagged(LyricPace.risesAtMost, by: LyricPace.risePerLine, atMost: LyricPace.risesAtMost)
        return now - arrived < LyricPace.rise + lastRise
    }

    private func swell(at now: TimeInterval) -> Double {
        let since = arrived.map { now - $0 } ?? 0
        return 0.5 + 0.5 * sin(since / LyricPace.breath * 2 * .pi)
    }

    private func breath(at index: Int, now: TimeInterval) -> LyricBreath? {
        if let waiting = frame?.waiting, playingHere, Int(waiting.next) == index {
            let opacity = breathing.flatMap { $0.line == index ? $0.opacity(at: now) : nil } ?? 1
            return LyricBreath(through: Double(waiting.through), opacity: opacity)
        }
        return fading.flatMap { faded in
            faded.line == index && !faded.settled(at: now) ? LyricBreath(through: 1, opacity: faded.fading(at: now)) : nil
        }
    }

    private func isBreathingInOrOut(at now: TimeInterval) -> Bool {
        breathing.map { !$0.settled(at: now) } ?? false || fading.map { !$0.settled(at: now) } ?? false
    }

    private func drawn(at now: TimeInterval, cue: LyricCue) -> LyricScene {
        let inks = cue.inks
        let column = min(max(pane.width - 2 * measures.gutter, 0), measures.column)
        let inside = max(column - 2 * measures.pad, 0)
        let twoVoices = cue.sheet.twoVoices
        let swell = swell(at: now)
        let seekable = playingHere && synced

        let window = cue.culls && placed ? visibleWindow : nil
        let measured = LyricColumn(width: pane.width, measures: measures)
        let rows = cue.sheet.lines.enumerated().map { index, line -> LyricSceneRow in
            if let window, let height = culledHeight(index, in: window, measured: measured) {
                return culledRow(index, height: height, column: column)
            }
            return row(index, line, now: now, cue: cue, column: column, inside: inside, twoVoices: twoVoices, swell: swell, seekable: seekable)
        }
        let end = synced ? ending(at: now, still: cue.still, column: column, inks: inks) : nil
        let edge = synced ? pane.height / 2 : measures.margin

        return LyricScene(
            generation: generation,
            rows: rows,
            end: end,
            offset: CGFloat(snapped(offset)),
            shown: arrival(at: now),
            head: edge,
            tail: edge,
            offersFollow: synced && playingHere && placed && !following(at: now)
        )
    }

    private var visibleWindow: ClosedRange<Double> {
        let beyond = max(Double(pane.height) * LyricPace.cullsBeyondPanes, Double(LyricPace.cullsWithin))
        return -beyond...(Double(pane.height) + beyond)
    }

    private func culledHeight(_ index: Int, in window: ClosedRange<Double>, measured: LyricColumn) -> CGFloat? {
        guard let frame = rows[index], columns[index] == measured else { return nil }
        let top = Double(frame.minY) + offset
        let bottom = Double(frame.maxY) + offset
        return bottom < window.lowerBound || top > window.upperBound ? frame.height : nil
    }

    private func culledRow(_ index: Int, height: CGFloat, column: CGFloat) -> LyricSceneRow {
        let look = LyricRowLook(
            culled: height,
            blank: true,
            text: "",
            words: .plain("", .init(red: 0, green: 0, blue: 0, opacity: 0)),
            side: .centre,
            width: column,
            textWidth: 0,
            standing: 0,
            breathes: false,
            breath: nil
        )
        return LyricSceneRow(id: index, look: look, inset: 0, moment: nil, lit: false)
    }

    private func row(
        _ index: Int,
        _ line: LyricRow,
        now: TimeInterval,
        cue: LyricCue,
        column: CGFloat,
        inside: CGFloat,
        twoVoices: Bool,
        swell: Double,
        seekable: Bool
    ) -> LyricSceneRow {
        guard !line.blank else {
            let look = LyricRowLook(
                culled: nil,
                blank: true,
                text: "",
                words: .plain("", cue.inks.muted),
                side: .centre,
                width: column,
                textWidth: 0,
                standing: 0,
                breathes: false,
                breath: nil
            )
            return LyricSceneRow(id: index, look: look, inset: 0, moment: nil, lit: false)
        }

        let second = line.voice == 1
        let side: LyricSide = twoVoices ? (second ? .trailing : .leading) : .centre
        let lit = second ? cue.inks.second : cue.inks.lit
        let lead = lead(index, at: now)
        let sung = LyricInks.mixed(cue.inks.muted, lit, by: lead)
        let waiting = LyricInks.mixed(LyricInks.thinned(lit, to: 0), lit, by: cue.inks.unsungShare)
        let unsung = LyricInks.mixed(cue.inks.muted, waiting, by: lead)
        let words = wipes[index].map { LyricWords.swept(line.text, by: $0, unsung: unsung, sung: sung) }
            ?? .plain(line.text, sung)
        let breath = breath(at: index, now: now)
        let standing = max(standing(index, at: now), cue.inks.standingFloor)
        let look = LyricRowLook(
            culled: nil,
            blank: false,
            text: line.text,
            words: words,
            side: side,
            width: column,
            textWidth: inside * (twoVoices ? LyricPace.voiceOfTwoSpans : 1),
            standing: breath.map { standing + (LyricPace.lit - standing) * $0.through } ?? standing,
            breathes: line.breathes,
            breath: breath.map { LyricBreathLook(through: $0.through, swell: swell, opacity: $0.opacity, ink: cue.inks.accent) }
        )

        return LyricSceneRow(
            id: index,
            look: look,
            inset: inset(index, at: now, still: cue.still),
            moment: seekable ? line.atMs : nil,
            lit: lead > 0.5
        )
    }

    private func ending(at now: TimeInterval, still: Bool, column: CGFloat, inks: LyricInks) -> LyricSceneEnd? {
        guard let sheet else { return nil }
        let end = Int(sheet.end)
        let look = LyricEndLook(standing: max(standing(end, at: now), inks.standingFloor), width: column, muted: inks.muted, faint: inks.faint)
        return LyricSceneEnd(id: end, look: look, inset: inset(end, at: now, still: still))
    }
}
