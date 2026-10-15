import Foundation
import Observation
import ResonateNative
import SwiftUI

extension EqualiserFilter {
    static let every: [EqualiserFilter] = [
        .peaking, .lowShelf, .highShelf, .lowPass, .highPass, .notch, .bandPass, .allPass,
    ]

    var label: String {
        switch self {
        case .peaking: "Peak"
        case .lowShelf: "Low shelf"
        case .highShelf: "High shelf"
        case .lowPass: "Low pass"
        case .highPass: "High pass"
        case .notch: "Notch"
        case .bandPass: "Band pass"
        case .allPass: "All pass"
        }
    }

    var usesGain: Bool {
        switch self {
        case .peaking, .lowShelf, .highShelf: true
        case .lowPass, .highPass, .notch, .bandPass, .allPass: false
        }
    }
}

extension EqualiserCurve {
    static let flat = EqualiserCurve(name: "", preampDb: 0, bands: [])

    var summary: String {
        if !name.isEmpty { return name }
        return bands.isEmpty ? "Flat" : "Custom curve · \(counted(bands.count, "band"))"
    }

    func level(of band: EqualiserBand) -> Float {
        band.filter.usesGain ? band.gainDb + preampDb : preampDb
    }

    func replacing(_ index: Int, with band: EqualiserBand) -> EqualiserCurve {
        var next = self
        if next.bands.indices.contains(index) { next.bands[index] = band }
        return next
    }
}

enum EqualiserUnits {
    static let semitone = Float(pow(2.0, 1.0 / 12.0))
    static let qNotch = Float(pow(2.0, 1.0 / 6.0))
    static let gainStep: Float = 0.5
    static let addedQ: Float = 1.0

    static func hertz(_ frequency: Float) -> String {
        frequency >= 1000
            ? String(format: frequency >= 10_000 ? "%.1f kHz" : "%.2f kHz", frequency / 1000)
            : String(format: frequency >= 100 ? "%.0f Hz" : "%.1f Hz", frequency)
    }

    static func decibels(_ gain: Float) -> String {
        String(format: "%+.1f dB", gain)
    }

    static func q(_ q: Float) -> String {
        String(format: "Q %.2f", q)
    }

    static func tidy(hertz: Float) -> Float {
        switch hertz {
        case ..<100: (hertz * 10).rounded() / 10
        case ..<1000: hertz.rounded()
        default: (hertz / 10).rounded() * 10
        }
    }

    static func tidy(decibels: Float) -> Float {
        (decibels * 10).rounded() / 10
    }

    static func tidy(q: Float) -> Float {
        (q * 100).rounded() / 100
    }
}

struct EqualiserScale {
    static let lowestHz: Float = 20
    static let highestHz: Float = 20_000
    static let narrowestSpan: Float = 15
    static let widestSpan: Float = 30
    static let markedEvery: Float = 5
    static let labelledFrequencies: [(hertz: Float, label: String)] = [(100, "100"), (1000, "1k"), (10_000, "10k")]
    static let fainterFrequencies: [Float] = [
        30, 40, 50, 60, 70, 80, 90, 200, 300, 400, 500, 600, 700, 800, 900,
        2000, 3000, 4000, 5000, 6000, 7000, 8000, 9000,
    ]

    let size: CGSize
    let span: Float

    static func span(around response: [Float], handles: [Float]) -> Float {
        let loudest = (response + handles).lazy.filter(\.isFinite).map(abs).max() ?? 0
        let rounded = (loudest / markedEvery).rounded(.up) * markedEvery
        return min(max(rounded, narrowestSpan), widestSpan)
    }

    func x(_ hertz: Float) -> CGFloat {
        CGFloat(log10(hertz / Self.lowestHz) / log10(Self.highestHz / Self.lowestHz)) * size.width
    }

    func hertz(atX x: CGFloat) -> Float {
        let fraction = Float(min(max(x / max(size.width, 1), 0), 1))
        return Self.lowestHz * pow(Self.highestHz / Self.lowestHz, fraction)
    }

    func y(_ decibels: Float) -> CGFloat {
        let held = min(max(decibels, -span), span)
        return CGFloat((span - held) / (2 * span)) * size.height
    }

    func decibels(atY y: CGFloat) -> Float {
        span - Float(y / max(size.height, 1)) * 2 * span
    }

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

@MainActor
@Observable
final class EqualiserResponse {
    nonisolated static let points: UInt32 = 256
    private static let between: Duration = .milliseconds(30)

    private struct Asked: Equatable {
        let curve: EqualiserCurve
        let rate: UInt32?
    }

    private(set) var decibels: [Float] = []
    @ObservationIgnored private var asked: Asked?
    @ObservationIgnored private var drawn: Asked?
    @ObservationIgnored private var working = false

    func follow(_ curve: EqualiserCurve, rate: UInt32?) {
        let next = Asked(curve: curve, rate: rate)
        guard next != drawn else { return }
        asked = next
        guard !working else { return }
        working = true
        Task { await drain() }
    }

    private func drain() async {
        while let next = asked {
            asked = nil
            let curve = next.curve
            let rate = next.rate
            if let traced = try? await Work.aside({
                try responseCurve(curve: curve, points: Self.points, rateHz: rate)
            }) {
                decibels = traced
                drawn = next
            }
            if asked != nil { try? await Task.sleep(for: Self.between) }
        }
        working = false
    }
}

struct EqualiserGraph: View {
    let curve: EqualiserCurve
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
            .foregroundStyle(chosen ? Color.white : Color.worn)
            .frame(width: Self.handleSize, height: Self.handleSize)
            .background(Circle().fill(chosen ? Color.worn : Color(.systemBackground)))
            .overlay(Circle().strokeBorder(Color.worn, lineWidth: 2))
            .shadow(color: .black.opacity(0.15), radius: 2, y: 1)
            .scaleEffect(heldBand == index ? 1.25 : 1)
            .animation(.snappy(duration: 0.15), value: heldBand == index)
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
        commit(curve.replacing(index, with: band))
    }

    private func dragEnded() {
        guard let ended = touch else { return }
        touch = nil
        holding = pinched != nil
        if case .band(let held) = ended, held.moved, pinched == nil { discard() }
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
                edit(placed(held, at: value.location, scale: scale))
            }
            .onEnded { value in
                defer {
                    touch = nil
                    holding = pinched != nil
                }
                switch touch {
                case .band(let held):
                    releases += 1
                    if held.moved, pinched == nil { commit(placed(held, at: value.location, scale: scale)) }
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
            .onChanged { value in
                if pinched == nil {
                    guard let band = heldBand ?? selected, curve.bands.indices.contains(band) else { return }
                    pinched = Pinched(band: band, q: curve.bands[band].q)
                    holding = true
                    grabs += 1
                }
                guard let pinched else { return }
                edit(narrowed(pinched, by: value.magnification))
            }
            .onEnded { value in
                guard let ended = pinched else { return }
                pinched = nil
                holding = touch != nil
                releases += 1
                commit(narrowed(ended, by: value.magnification))
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
