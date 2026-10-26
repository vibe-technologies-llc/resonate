import ResonateLogic
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
    let addLyrics: (() -> Void)?

    init(track: Track?, compact: Bool = false, addLyrics: (() -> Void)? = nil) {
        self.track = track
        self.compact = compact
        self.addLyrics = addLyrics
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
            } else if let track, sheet == nil, !features.lyricsLoading, features.lyricsUnread == track.id {
                ContentUnavailableView {
                    Label("Lyrics could not be read", systemImage: "exclamationmark.bubble")
                } actions: {
                    Button("Try again", systemImage: "arrow.clockwise") { features.loadLyrics(track.id) }
                        .buttonStyle(.glass)
                    if let addLyrics {
                        Button("Add lyrics…", systemImage: "text.badge.plus", action: addLyrics)
                            .buttonStyle(.glass)
                    }
                }
            } else if features.lyricsLoading || sheet == nil {
                looking
            } else {
                ContentUnavailableView {
                    Label("No lyrics", systemImage: "quote.bubble")
                } description: {
                    Text(
                        features.online
                            ? "No lyrics found for this song"
                            : "No saved lyrics. Enable online features to look them up, or add an LRC beside the song."
                    )
                } actions: {
                    if let addLyrics {
                        Button("Add lyrics…", systemImage: "text.badge.plus", action: addLyrics)
                            .buttonStyle(.glass)
                    }
                }
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
    @Environment(\.scenePhase) private var phase
    @ScaledMetric(relativeTo: .title) private var fullWords: CGFloat = 32
    @ScaledMetric(relativeTo: .title2) private var compactWords: CGFloat = 28
    @State private var offersFollow = false
    @State private var hits = LyricHits()
    @GestureState private var held = false

    let sheet: LyricSheet
    let track: Track?
    let stage: LyricStage
    let visible: Bool
    let compact: Bool

    var body: some View {
        let heard = LyricHeard.of(library.snapshot, showing: track)
        let measures = LyricMeasures.at(words: compact ? compactWords : fullWords, compact: compact)
        let inks = LyricInks(
            scheme: scheme,
            accent: Color.worn.resolve(in: Self.resolving(scheme, contrast)),
            legibility: contrast == .increased || reduceTransparency ? .solid : .glass
        )
        let awake = visible && phase == .active
        let sounding = heard?.playing == true && sheet.detail != .unsynced
        let setting = LyricSetting(
            sheet: sheet,
            heard: heard,
            clock: library.clock,
            native: library.native,
            measures: measures,
            inks: inks,
            still: reduceMotion,
            awake: awake,
            sweeps: awake && sounding && sheet.detail == .words,
            ticks: awake && sounding
        )

        return LyricLayers(setting: setting, stage: stage, beat: stage.beat, hits: hits, seek: seek) { offersFollow = $0 }
            .contentShape(.rect)
            .simultaneousGesture(hand)
            .simultaneousGesture(SpatialTapGesture().onEnded { tap in
                if let moment = hits.moment(tap.location) { seek(to: moment) }
            })
            .onChange(of: held) { _, holding in
                guard !holding else { return }
                Task { stage.letGo(at: Date.now.timeIntervalSinceReferenceDate) }
            }
            .overlay(alignment: .bottom) { follow }
            .animation(.easeInOut(duration: LyricPace.turn), value: offersFollow)
    }

    private static func resolving(_ scheme: ColorScheme, _ contrast: ColorSchemeContrast) -> EnvironmentValues {
        var values = EnvironmentValues()
        values.colorScheme = scheme
        values._colorSchemeContrast = contrast
        return values
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

    @ViewBuilder private var follow: some View {
        if offersFollow {
            Button {
                stage.followAgain(at: Date.now.timeIntervalSinceReferenceDate)
            } label: {
                Label("Follow", systemImage: "quote.bubble")
            }
            .buttonStyle(.glass)
            .controlSize(compact ? .small : .regular)
            .padding(.bottom, compact ? 6 : 12)
            .transition(.opacity)
            .accessibilityHint("Scrolls back to the words being sung")
        }
    }

    private func seek(to moment: UInt64) {
        stage.followAgain(at: Date.now.timeIntervalSinceReferenceDate)
        library.request { try $0.seek(positionMs: moment) }
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
}

enum LyricWords: Equatable {
    case plain(Color.Resolved)
    case swept(LyricWipe, unsung: Color.Resolved, sung: Color.Resolved)
}

struct LyricBreathLook: Equatable {
    let through: Double
    let swell: Double
    let opacity: Double
    let ink: Color.Resolved
}

struct LyricRowLook: Equatable {
    let blank: Bool
    let text: String
    let words: LyricWords
    let standing: Double
    let breathes: Bool
    let breath: LyricBreathLook?
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

        if frame?.wipes != landed.wipes {
            wipes = Dictionary(landed.wipes.map { (Int($0.line), $0) }) { kept, _ in kept }
        }
        frame = landed
        return moved
    }

    func measured(row: Int, _ frame: CGRect, generation: Int) {
        guard generation == self.generation else { return }
        rows[row] = frame
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
        let swell = swell(at: now)
        let seekable = playingHere && synced
        let rows = cue.sheet.lines.enumerated().map { index, line in
            row(index, line, now: now, cue: cue, swell: swell, seekable: seekable)
        }
        let end = synced ? ending(at: now, still: cue.still, inks: cue.inks) : nil

        return LyricScene(
            generation: generation,
            rows: rows,
            end: end,
            offset: CGFloat(snapped(offset)),
            shown: arrival(at: now),
            offersFollow: synced && playingHere && placed && !following(at: now)
        )
    }

    private func row(_ index: Int, _ line: LyricRow, now: TimeInterval, cue: LyricCue, swell: Double, seekable: Bool) -> LyricSceneRow {
        guard !line.blank else {
            let look = LyricRowLook(blank: true, text: "", words: .plain(cue.inks.muted), standing: 0, breathes: false, breath: nil)
            return LyricSceneRow(id: index, look: look, inset: 0, moment: nil, lit: false)
        }

        let lit = line.voice == 1 ? cue.inks.second : cue.inks.lit
        let lead = lead(index, at: now)
        let sung = LyricInks.mixed(cue.inks.muted, lit, by: lead)
        let waiting = LyricInks.mixed(LyricInks.thinned(lit, to: 0), lit, by: cue.inks.unsungShare)
        let unsung = LyricInks.mixed(cue.inks.muted, waiting, by: lead)
        let words = wipes[index].map { LyricWords.swept($0, unsung: unsung, sung: sung) } ?? .plain(sung)
        let breath = breath(at: index, now: now)
        let standing = max(standing(index, at: now), cue.inks.standingFloor)
        let look = LyricRowLook(
            blank: false,
            text: line.text,
            words: words,
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

    private func ending(at now: TimeInterval, still: Bool, inks: LyricInks) -> LyricSceneEnd? {
        guard let sheet else { return nil }
        let end = Int(sheet.end)
        let look = LyricEndLook(standing: max(standing(end, at: now), inks.standingFloor), muted: inks.muted, faint: inks.faint)
        return LyricSceneEnd(id: end, look: look, inset: inset(end, at: now, still: still))
    }
}
