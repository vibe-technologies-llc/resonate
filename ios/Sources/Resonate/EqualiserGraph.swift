import Foundation
import Observation
import ResonateLogic
import ResonateNative
import SwiftUI

extension EqualiserPreset {
    var mark: String {
        switch name {
        case "Flat": "minus"
        case "Bass boost": "speaker.wave.3"
        case "Bass only": "hifispeaker"
        case "Bass cut": "water.waves"
        case "Vocals": "music.mic"
        case "Warm": "sun.max"
        case "Bright": "sparkles"
        case "Rock": "guitars"
        case "Electronic": "waveform"
        default: "slider.horizontal.3"
        }
    }
}

struct EqualiserPresetGrid: View {
    let presets: [EqualiserPreset]
    let chosen: String?
    let choose: (EqualiserPreset) -> Void
    @ScaledMetric private var narrowest: CGFloat = 112

    var body: some View {
        LazyVGrid(columns: [GridItem(.adaptive(minimum: narrowest), spacing: 8)], spacing: 8) {
            ForEach(presets, id: \.name) { preset in
                let isChosen = preset.name == chosen
                Button {
                    choose(preset)
                } label: {
                    Label(preset.name, systemImage: preset.mark)
                        .font(.subheadline.weight(.medium))
                        .lineLimit(1)
                        .minimumScaleFactor(0.8)
                        .padding(.horizontal, 10)
                        .frame(maxWidth: .infinity, minHeight: 44)
                        .foregroundStyle(isChosen ? Color.wornInk : Color.primary)
                        .background(isChosen ? Color.worn : Color.primary.opacity(0.08), in: .capsule)
                        .contentShape(.capsule)
                }
                .buttonStyle(.plain)
                .accessibilityHint(preset.detail)
                .accessibilityAddTraits(isChosen ? .isSelected : [])
            }
        }
        .padding(.vertical, 6)
    }
}

struct EqualiserSliders: View {
    let curve: EqualiserCurve
    let limits: EqualiserLimits
    let change: (Int, Float) -> Void
    let open: (Int) -> Void

    var body: some View {
        ForEach(Array(curve.bands.enumerated()), id: \.offset) { index, band in
            if band.filter.usesGain {
                BandGainSlider(band: band, limits: limits) { change(index, $0) }
            } else {
                HStack {
                    VStack(alignment: .leading, spacing: 2) {
                        Text(EqualiserUnits.hertz(band.frequencyHz))
                        Text(band.filter.label).font(.footnote).foregroundStyle(.secondary)
                    }
                    Spacer()
                    Button("Edit") { open(index) }
                }
            }
        }
    }
}

struct BandGainSlider: View {
    let band: EqualiserBand
    let limits: EqualiserLimits
    let commit: (Float) -> Void
    @State private var editing: Float?
    private static let shownAtLeastDb: Float = 12

    var body: some View {
        let hertz = EqualiserUnits.hertz(band.frequencyHz)
        let range = EqualiserUnits.range(of: band.frequencyHz)
        let reach = min(limits.gainAtMostDb, max(Self.shownAtLeastDb, abs(band.gainDb)))
        VStack(alignment: .leading, spacing: 2) {
            HStack {
                Text(range).font(.subheadline.weight(.semibold))
                Spacer()
                Text(EqualiserUnits.decibels(editing ?? band.gainDb))
                    .font(.subheadline.weight(.semibold))
                    .monospacedDigit()
                    .foregroundStyle(Color.worn)
            }
            Text(band.filter == .peaking ? hertz : "\(hertz) · \(band.filter.label)")
                .font(.footnote)
                .foregroundStyle(.secondary)
            CommittingSlider(
                title: "\(hertz) \(range)",
                value: band.gainDb,
                range: -reach...reach,
                step: EqualiserUnits.gainStep,
                editing: $editing,
                spoken: { String(format: "%+.1f decibels", Double($0)) },
                commit: commit
            )
        }
    }
}


struct EqualiserGraph: View {
    let given: EqualiserCurve
    let response: [Float]
    let limits: EqualiserLimits
    @Binding var selected: Int?
    @Binding var holding: Bool
    let edit: (EqualiserCurve) -> Void
    let commit: (EqualiserCurve) -> Void
    let add: (EqualiserBand) -> Void
    let discard: () -> Void

    private static let reach: CGFloat = 32
    private static let tapTravel: CGFloat = 8
    private static let handleSize: CGFloat = 26
    private static let smallestPinch: CGFloat = 0.05
    private static let toldEvery: Duration = .milliseconds(50)

    private struct Held {
        let band: Int
        let offset: CGSize
        let span: Float
        var moved = false
    }

    private struct Pinched {
        let band: Int
        let q: Float
    }

    private enum Touch {
        case band(Held)
        case empty(CGPoint)
    }

    @State private var touch: Touch?
    @State private var pinched: Pinched?
    @State private var grabs = 0
    @State private var releases = 0
    @GestureState private var dragged = false
    @GestureState private var pinchHeld = false
    @State private var moving: EqualiserCurve?
    @State private var toldAt = ContinuousClock.now
    @State private var telling: Task<Void, Never>?

    init(
        curve: EqualiserCurve,
        response: [Float],
        limits: EqualiserLimits,
        selected: Binding<Int?>,
        holding: Binding<Bool>,
        edit: @escaping (EqualiserCurve) -> Void,
        commit: @escaping (EqualiserCurve) -> Void,
        add: @escaping (EqualiserBand) -> Void,
        discard: @escaping () -> Void
    ) {
        given = curve
        self.response = response
        self.limits = limits
        _selected = selected
        _holding = holding
        self.edit = edit
        self.commit = commit
        self.add = add
        self.discard = discard
    }

    private var curve: EqualiserCurve { moving ?? given }

    var body: some View {
        GeometryReader { geometry in
            let scale = EqualiserScale(size: geometry.size, span: frozenSpan ?? currentSpan)
            ZStack(alignment: .topLeading) {
                Canvas { context, _ in draw(in: &context, scale: scale) }
                    .accessibilityHidden(true)
                ForEach(curve.bands.indices, id: \.self) { index in
                    handle(index, scale: scale)
                }
            }
            .contentShape(Rectangle())
            .gesture(dragging(scale))
            .simultaneousGesture(pinching)
        }
        .onChange(of: dragged) { _, now in
            if !now { dragEnded() }
        }
        .onChange(of: pinchHeld) { _, now in
            if !now { pinchEnded() }
        }
        .sensoryFeedback(.impact(weight: .light), trigger: grabs)
        .sensoryFeedback(.selection, trigger: releases)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Equaliser curve")
        .accessibilityAction(named: "Add a band") { addAt(hertz: 1000, decibels: 0) }
    }

    private var currentSpan: Float {
        EqualiserScale.span(around: response, handles: curve.bands.map { curve.level(of: $0) })
    }

    private var frozenSpan: Float? {
        if case .band(let held) = touch { return held.span }
        return nil
    }

    private func draw(in context: inout GraphicsContext, scale: EqualiserScale) {
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
                    Text(zero ? "0 dB" : String(format: "%+.0f", level)).font(.caption2).foregroundStyle(.secondary),
                    at: CGPoint(x: 3, y: y + (level == scale.span ? 2 : -2)),
                    anchor: level == scale.span ? .topLeading : .bottomLeading
                )
            }
            level += EqualiserScale.markedEvery
        }

        guard response.count > 1 else { return }
        context.fill(scale.washed(response), with: .color(Color.worn.opacity(0.18)))
        context.stroke(
            scale.traced(response),
            with: .color(Color.worn),
            style: StrokeStyle(lineWidth: 2, lineCap: .round, lineJoin: .round)
        )
    }

    private func handle(_ index: Int, scale: EqualiserScale) -> some View {
        let band = curve.bands[index]
        let chosen = selected == index
        return Text("\(index + 1)")
            .font(.caption2.weight(.bold).monospacedDigit())
            .foregroundStyle(chosen ? Color.wornInk : Color.worn)
            .frame(width: Self.handleSize, height: Self.handleSize)
            .background(Circle().fill(chosen ? Color.worn : Color(.systemBackground)))
            .overlay(Circle().strokeBorder(Color.worn, lineWidth: 2))
            .shadow(color: .black.opacity(0.15), radius: 2, y: 1)
            .scaleEffect(heldBand == index ? 1.25 : 1)
            .motion(.snappy(duration: 0.15), value: heldBand == index)
            .position(x: scale.x(band.frequencyHz), y: scale.y(curve.level(of: band)))
            .allowsHitTesting(false)
            .accessibilityElement()
            .accessibilityLabel("Band \(index + 1), \(band.filter.label) at \(EqualiserUnits.hertz(band.frequencyHz))")
            .accessibilityValue(
                band.filter.usesGain
                    ? "\(EqualiserUnits.decibels(band.gainDb)), \(EqualiserUnits.q(band.q))"
                    : EqualiserUnits.q(band.q)
            )
            .accessibilityAddTraits(chosen ? [.isButton, .isSelected] : .isButton)
            .accessibilityAction { selected = index }
            .accessibilityAdjustableAction { direction in
                nudge(index, by: direction == .increment ? EqualiserUnits.gainStep : -EqualiserUnits.gainStep)
            }
    }

    private var heldBand: Int? {
        if case .band(let held) = touch { return held.band }
        return pinched?.band
    }

    private func nudge(_ index: Int, by step: Float) {
        guard curve.bands.indices.contains(index) else { return }
        var band = curve.bands[index]
        if band.filter.usesGain {
            band.gainDb = min(max(band.gainDb + step, -limits.gainAtMostDb), limits.gainAtMostDb)
        } else {
            let factor = step > 0 ? EqualiserUnits.qNotch : 1 / EqualiserUnits.qNotch
            band.q = min(max(band.q * factor, limits.qAtLeast), limits.qAtMost)
        }
        selected = index
        kept(curve.replacing(index, with: band))
    }

    private func told(_ next: EqualiserCurve) {
        moving = next
        let now = ContinuousClock.now
        if toldAt.duration(to: now) >= Self.toldEvery {
            toldAt = now
            edit(next)
            return
        }
        guard telling == nil else { return }
        telling = Task {
            try? await Task.sleep(for: Self.toldEvery)
            telling = nil
            guard let moving else { return }
            toldAt = .now
            edit(moving)
        }
    }

    private func settled() {
        telling?.cancel()
        telling = nil
        moving = nil
    }

    private func kept(_ next: EqualiserCurve) {
        settled()
        commit(next)
    }

    private func pinchEnded() {
        guard pinched != nil else { return }
        pinched = nil
        holding = touch != nil
        settled()
        discard()
    }

    private func dragEnded() {
        guard let ended = touch else { return }
        touch = nil
        holding = pinched != nil
        if case .band(let held) = ended, held.moved, pinched == nil {
            settled()
            discard()
        }
    }

    private func dragging(_ scale: EqualiserScale) -> some Gesture {
        DragGesture(minimumDistance: 0, coordinateSpace: .local)
            .updating($dragged) { _, state, _ in state = true }
            .onChanged { value in
                if touch == nil { touch = touched(at: value.startLocation, scale: scale) }
                guard pinched == nil, case .band(var held) = touch else { return }
                let travelled = hypot(value.translation.width, value.translation.height)
                guard held.moved || travelled > 2 else { return }
                held.moved = true
                touch = .band(held)
                told(placed(held, at: value.location, scale: scale))
            }
            .onEnded { value in
                defer {
                    touch = nil
                    holding = pinched != nil
                }
                switch touch {
                case .band(let held):
                    releases += 1
                    if held.moved, pinched == nil { kept(placed(held, at: value.location, scale: scale)) }
                case .empty(let start):
                    let travelled = hypot(value.location.x - start.x, value.location.y - start.y)
                    guard pinched == nil, travelled < Self.tapTravel else { return }
                    addAt(hertz: scale.hertz(atX: start.x), decibels: scale.decibels(atY: start.y) - curve.preampDb)
                case nil:
                    return
                }
            }
    }

    private var pinching: some Gesture {
        MagnifyGesture()
            .updating($pinchHeld) { _, state, _ in state = true }
            .onChanged { value in
                if pinched == nil {
                    guard let band = heldBand ?? selected, curve.bands.indices.contains(band) else { return }
                    pinched = Pinched(band: band, q: curve.bands[band].q)
                    holding = true
                    grabs += 1
                }
                guard let pinched else { return }
                told(narrowed(pinched, by: value.magnification))
            }
            .onEnded { value in
                guard let ended = pinched else { return }
                pinched = nil
                holding = touch != nil
                releases += 1
                kept(narrowed(ended, by: value.magnification))
            }
    }

    private func placed(_ held: Held, at location: CGPoint, scale: EqualiserScale) -> EqualiserCurve {
        guard curve.bands.indices.contains(held.band) else { return curve }
        let pointed = CGPoint(x: location.x + held.offset.width, y: location.y + held.offset.height)
        return curve.replacing(held.band, with: moved(curve.bands[held.band], to: pointed, scale: scale))
    }

    private func narrowed(_ pinched: Pinched, by magnification: CGFloat) -> EqualiserCurve {
        guard curve.bands.indices.contains(pinched.band) else { return curve }
        var band = curve.bands[pinched.band]
        let widened = pinched.q / Float(max(magnification, Self.smallestPinch))
        band.q = EqualiserUnits.tidy(q: min(max(widened, limits.qAtLeast), limits.qAtMost))
        return curve.replacing(pinched.band, with: band)
    }

    private func touched(at point: CGPoint, scale: EqualiserScale) -> Touch {
        let nearest = curve.bands.indices
            .map { index -> (Int, CGPoint) in
                let band = curve.bands[index]
                return (index, CGPoint(x: scale.x(band.frequencyHz), y: scale.y(curve.level(of: band))))
            }
            .min { hypot($0.1.x - point.x, $0.1.y - point.y) < hypot($1.1.x - point.x, $1.1.y - point.y) }
        guard let (index, centre) = nearest, hypot(centre.x - point.x, centre.y - point.y) <= Self.reach else {
            return .empty(point)
        }
        selected = index
        holding = true
        grabs += 1
        return .band(Held(
            band: index,
            offset: CGSize(width: centre.x - point.x, height: centre.y - point.y),
            span: scale.span
        ))
    }

    private func moved(_ band: EqualiserBand, to point: CGPoint, scale: EqualiserScale) -> EqualiserBand {
        var next = band
        let hertz = EqualiserUnits.tidy(hertz: scale.hertz(atX: point.x))
        next.frequencyHz = min(max(hertz, limits.lowestHz), limits.highestHz)
        if band.filter.usesGain {
            let gain = EqualiserUnits.tidy(decibels: scale.decibels(atY: point.y) - curve.preampDb)
            next.gainDb = min(max(gain, -limits.gainAtMostDb), limits.gainAtMostDb)
        }
        return next
    }

    private func addAt(hertz: Float, decibels: Float) {
        guard curve.bands.count < Int(limits.bandsAtMost) else { return }
        add(EqualiserBand(
            filter: .peaking,
            frequencyHz: min(max(EqualiserUnits.tidy(hertz: hertz), limits.lowestHz), limits.highestHz),
            gainDb: min(max(EqualiserUnits.tidy(decibels: decibels), -limits.gainAtMostDb), limits.gainAtMostDb),
            q: EqualiserUnits.addedQ
        ))
    }
}

struct BandInspector: View {
    let band: EqualiserBand
    let number: Int
    let limits: EqualiserLimits
    let change: (EqualiserBand) -> Void
    let delete: () -> Void

    var body: some View {
        Section("Band \(number)") {
            Picker("Type", selection: Binding(get: { band.filter }, set: { filter in
                var next = band
                next.filter = filter
                change(next)
            })) {
                ForEach(EqualiserFilter.every, id: \.self) { filter in
                    Text(filter.label).tag(filter)
                }
            }
            BandValueRow(
                title: "Frequency",
                unit: "Hz",
                value: band.frequencyHz,
                range: limits.lowestHz...limits.highestHz,
                stepped: { value, up in EqualiserUnits.tidy(hertz: up ? value * EqualiserUnits.semitone : value / EqualiserUnits.semitone) }
            ) { hertz in
                var next = band
                next.frequencyHz = hertz
                change(next)
            }
            if band.filter.usesGain {
                BandValueRow(
                    title: "Gain",
                    unit: "dB",
                    value: band.gainDb,
                    range: -limits.gainAtMostDb...limits.gainAtMostDb,
                    stepped: { value, up in EqualiserUnits.tidy(decibels: value + (up ? EqualiserUnits.gainStep : -EqualiserUnits.gainStep)) }
                ) { gain in
                    var next = band
                    next.gainDb = gain
                    change(next)
                }
            }
            BandValueRow(
                title: "Q",
                unit: "",
                value: band.q,
                range: limits.qAtLeast...limits.qAtMost,
                stepped: { value, up in EqualiserUnits.tidy(q: up ? value * EqualiserUnits.qNotch : value / EqualiserUnits.qNotch) }
            ) { q in
                var next = band
                next.q = q
                change(next)
            }
            Button("Delete band", systemImage: "trash", role: .destructive, action: delete)
        }
    }
}

struct BandValueRow: View {
    let title: String
    let unit: String
    let value: Float
    let range: ClosedRange<Float>
    let stepped: (Float, Bool) -> Float
    let commit: (Float) -> Void

    var body: some View {
        HStack {
            Text(title)
            Spacer()
            TextField(
                title,
                value: Binding(get: { Double(value) }, set: { typed in held(Float(typed)) }),
                format: .number.precision(.fractionLength(0...2))
            )
            .keyboardType(.numbersAndPunctuation)
            .multilineTextAlignment(.trailing)
            .monospacedDigit()
            .frame(maxWidth: 96)
            if !unit.isEmpty {
                Text(unit).foregroundStyle(.secondary)
            }
            Stepper(title) {
                held(stepped(value, true))
            } onDecrement: {
                held(stepped(value, false))
            }
            .labelsHidden()
        }
    }

    private func held(_ chosen: Float) {
        guard chosen.isFinite else { return }
        let bounded = min(max(chosen, range.lowerBound), range.upperBound)
        guard bounded != value else { return }
        commit(bounded)
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
