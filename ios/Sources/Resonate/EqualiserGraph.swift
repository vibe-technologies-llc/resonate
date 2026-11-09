import Foundation
import Observation
import ResonateLogic
import ResonateNative
import SwiftUI

extension EqualiserPreset {
    private static let marks = [
        "minus", "speaker.wave.3", "hifispeaker", "water.waves", "music.mic", "sun.max", "rays", "cloud", "guitars", "waveform",
    ]

    static func mark(at place: Int) -> String {
        marks.indices.contains(place) ? marks[place] : "slider.horizontal.3"
    }
}

struct EqualiserGraph: View {
    static let space = "equaliser-graph"

    let curve: EqualiserCurve
    let response: EqualiserResponse
    let limits: EqualiserLimits
    let selected: Int?
    let choose: (Int) -> Void
    let edit: (EqualiserCurve) -> Void
    let commit: (EqualiserCurve) -> Void
    let discard: () -> Void

    private struct Held: Equatable {
        let band: Int
        let start: CGPoint
        let span: Float
    }

    @State private var held: Held?
    @State private var moving: EqualiserCurve?
    @State private var pace = TellingPace()
    @State private var telling: Task<Void, Never>?
    @State private var grabs = 0
    @State private var releases = 0

    var body: some View {
        let drawn = moving ?? curve
        let decibels = response.decibels
        GeometryReader { geometry in
            let span = held?.span ?? EqualiserScale.span(around: decibels, handles: drawn.bands.map { drawn.level(of: $0) })
            let scale = EqualiserScale(size: geometry.size, span: span)
            ZStack(alignment: .topLeading) {
                EqualiserGrid(scale: scale)
                    .equatable()
                EqualiserTrace(scale: scale, decibels: decibels)
                    .equatable()
                ForEach(EqualiserHandles.laid(drawn, on: scale, selected: selected), id: \.band) { handle in
                    EqualiserHandleView(
                        handle: handle,
                        band: drawn.bands[handle.band],
                        chosen: handle.band == selected,
                        held: held?.band == handle.band,
                        grab: { grab(handle, span: scale.span) },
                        move: { move(handle.band, by: $0, on: scale) },
                        release: { release(handle.band, by: $0, on: scale) },
                        lose: { lose(handle.band) },
                        choose: { choose(handle.band) },
                        nudge: { nudged in kept(drawn.replacing(handle.band, with: nudged)) },
                        limits: limits
                    )
                    .zIndex(Double(handle.layer))
                }
            }
            .coordinateSpace(.named(Self.space))
        }
        .padding(EqualiserHandles.drawnEdge / 2)
        .sensoryFeedback(.impact(weight: .light), trigger: grabs)
        .sensoryFeedback(.selection, trigger: releases)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("equaliser.graph.curve")
    }

    private func grab(_ handle: EqualiserHandle, span: Float) {
        guard held?.band != handle.band else { return }
        held = Held(band: handle.band, start: handle.centre, span: span)
        grabs += 1
        choose(handle.band)
    }

    private func placed(_ band: Int, by translation: CGSize, on scale: EqualiserScale) -> EqualiserCurve? {
        guard let held, held.band == band, curve.bands.indices.contains(band) else { return nil }
        let moved = EqualiserDrag.moved(
            curve.bands[band], from: held.start, by: translation, on: scale, preamp: curve.preampDb, limits: limits
        )
        return curve.replacing(band, with: moved)
    }

    private func move(_ band: Int, by translation: CGSize, on scale: EqualiserScale) {
        guard moving != nil || EqualiserDrag.travelled(translation), let next = placed(band, by: translation, on: scale) else { return }
        moving = next
        tell(next)
    }

    private func release(_ band: Int, by translation: CGSize, on scale: EqualiserScale) {
        guard held?.band == band else { return }
        releases += 1
        let next = moving == nil ? nil : placed(band, by: translation, on: scale)
        held = nil
        if let next {
            kept(next)
        } else {
            settled()
        }
    }

    private func lose(_ band: Int) {
        guard held?.band == band else { return }
        held = nil
        let dropped = moving != nil
        settled()
        if dropped { discard() }
    }

    private func tell(_ next: EqualiserCurve) {
        guard let wait = pace.ask(at: .now) else {
            edit(next)
            return
        }
        guard telling == nil else { return }
        telling = Task {
            try? await Task.sleep(for: wait)
            telling = nil
            guard !Task.isCancelled, let moving else { return }
            pace.told(at: .now)
            edit(moving)
        }
    }

    private func settled() {
        telling?.cancel()
        telling = nil
        moving = nil
        pace.forget()
    }

    private func kept(_ next: EqualiserCurve) {
        settled()
        commit(next)
    }
}

private struct EqualiserHandleView: View {
    private static let shade = 0.15

    let handle: EqualiserHandle
    let band: EqualiserBand
    let chosen: Bool
    let held: Bool
    let grab: () -> Void
    let move: (CGSize) -> Void
    let release: (CGSize) -> Void
    let lose: () -> Void
    let choose: () -> Void
    let nudge: (EqualiserBand) -> Void
    let limits: EqualiserLimits
    @GestureState private var pressing = false

    var body: some View {
        Text("\(handle.band + 1)")
            .font(.caption2.weight(.bold).monospacedDigit())
            .foregroundStyle(chosen ? Color.wornInk : Color.worn)
            .frame(width: EqualiserHandles.drawnEdge, height: EqualiserHandles.drawnEdge)
            .background(Circle().fill(chosen ? Color.worn : Color(.systemBackground)))
            .overlay(Circle().strokeBorder(Color.worn, lineWidth: 2))
            .background(Circle().fill(Color.black.opacity(Self.shade)).padding(-1).offset(y: 1))
            .scaleEffect(held ? 1.25 : 1)
            .motion(.snappy(duration: 0.15), value: held)
            .frame(width: handle.reach.width, height: handle.reach.height)
            .contentShape(.rect)
            .accessibilityElement()
            .accessibilityLabel(said("equaliser.graph.band", handle.band + 1, band.filter.label, EqualiserUnits.hertz(band.frequencyHz)))
            .accessibilityValue(
                band.filter.usesGain
                    ? "\(EqualiserUnits.decibels(band.gainDb)), \(EqualiserUnits.q(band.q))"
                    : EqualiserUnits.q(band.q)
            )
            .accessibilityAddTraits(chosen ? [.isButton, .isSelected] : .isButton)
            .accessibilityAction { choose() }
            .accessibilityAdjustableAction { direction in
                nudge(BandNudge.level(band, up: direction == .increment, limits: limits))
            }
            .accessibilityActions {
                if band.filter.usesGain {
                    Button("equaliser.graph.gain.raise") { nudge(BandNudge.level(band, up: true, limits: limits)) }
                    Button("equaliser.graph.gain.lower") { nudge(BandNudge.level(band, up: false, limits: limits)) }
                }
                Button("equaliser.graph.frequency.raise") { nudge(BandNudge.frequency(band, up: true, limits: limits)) }
                Button("equaliser.graph.frequency.lower") { nudge(BandNudge.frequency(band, up: false, limits: limits)) }
            }
            .gesture(
                DragGesture(minimumDistance: 0, coordinateSpace: .named(EqualiserGraph.space))
                    .updating($pressing) { _, state, _ in state = true }
                    .onChanged { value in
                        grab()
                        move(value.translation)
                    }
                    .onEnded { value in release(value.translation) }
            )
            .onChange(of: pressing) { _, now in
                if !now { lose() }
            }
            .position(handle.centre)
    }
}

struct EqualiserTrace: View, Equatable {
    let scale: EqualiserScale
    let decibels: [Float]

    var body: some View {
        let tint = Color.worn
        Canvas { context, _ in
            guard decibels.count > 1 else { return }
            context.fill(scale.washed(decibels), with: .color(tint.opacity(0.18)))
            context.stroke(
                scale.traced(decibels),
                with: .color(tint),
                style: StrokeStyle(lineWidth: 2, lineCap: .round, lineJoin: .round)
            )
        }
        .accessibilityHidden(true)
    }

    nonisolated static func == (shown: EqualiserTrace, other: EqualiserTrace) -> Bool {
        shown.scale == other.scale && shown.decibels == other.decibels
    }
}

extension EqualiserScale {
    func traced(_ response: [Float]) -> Path {
        Path { path in
            guard response.count > 1 else { return }
            let last = CGFloat(response.count - 1)
            for (index, decibels) in response.enumerated() {
                let point = CGPoint(x: CGFloat(index) / last * size.width, y: y(decibels))
                if index == 0 { path.move(to: point) } else { path.addLine(to: point) }
            }
        }
    }

    func washed(_ response: [Float]) -> Path {
        var path = traced(response)
        guard response.count > 1 else { return path }
        path.addLine(to: CGPoint(x: size.width, y: y(0)))
        path.addLine(to: CGPoint(x: 0, y: y(0)))
        path.closeSubpath()
        return path
    }
}

extension EqualiserResponse {
    convenience init() {
        self.init(tracing: { curve, rate in
            try await Work.aside { try responseCurve(curve: curve, points: EqualiserResponse.points, rateHz: rate) }
        })
    }
}

struct EqualiserGrid: View, Equatable {
    let scale: EqualiserScale

    var body: some View {
        Canvas { context, _ in draw(in: &context) }
            .accessibilityHidden(true)
    }

    nonisolated static func == (shown: EqualiserGrid, other: EqualiserGrid) -> Bool {
        shown.scale == other.scale
    }

    private func draw(in context: inout GraphicsContext) {
        let size = scale.size
        let faint = Color.secondary.opacity(0.18)
        let marked = Color.secondary.opacity(0.4)

        for hertz in EqualiserScale.fainterFrequencies {
            let x = scale.x(hertz)
            context.stroke(Path { $0.move(to: CGPoint(x: x, y: 0)); $0.addLine(to: CGPoint(x: x, y: size.height)) }, with: .color(faint), lineWidth: 0.5)
        }
        for (hertz, label) in EqualiserScale.labelledFrequencies {
            let x = scale.x(hertz)
            context.stroke(Path { $0.move(to: CGPoint(x: x, y: 0)); $0.addLine(to: CGPoint(x: x, y: size.height)) }, with: .color(marked), lineWidth: 0.75)
            context.draw(
                Text(label).font(.caption2).foregroundStyle(.secondary),
                at: CGPoint(x: x + 3, y: size.height - 3),
                anchor: .bottomLeading
            )
        }

        var level = -scale.span
        while level <= scale.span {
            let y = scale.y(level)
            let zero = level == 0
            context.stroke(
                Path { $0.move(to: CGPoint(x: 0, y: y)); $0.addLine(to: CGPoint(x: size.width, y: y)) },
                with: .color(zero ? marked : faint),
                lineWidth: zero ? 1 : 0.5
            )
            if zero || abs(level) == scale.span {
                context.draw(
                    Text(zero ? said("equaliser.graph.zero-db") : String(format: "%+.0f", level)).font(.caption2).foregroundStyle(.secondary),
                    at: CGPoint(x: 3, y: y + (level == scale.span ? 2 : -2)),
                    anchor: level == scale.span ? .topLeading : .bottomLeading
                )
            }
            level += EqualiserScale.markedEvery
        }
    }
}
