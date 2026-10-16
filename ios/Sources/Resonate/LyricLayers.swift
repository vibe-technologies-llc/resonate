import ResonateNative
import SwiftUI
import UIKit

struct LyricSetting {
    let sheet: LyricSheet
    let playback: LyricPlayback?
    let native: MobilePlayer?
    let measures: LyricMeasures
    let inks: LyricInks
    let still: Bool
    let awake: Bool
    let sweeps: Bool
    let ticks: Bool
}

@MainActor
final class LyricHits {
    var moment: (CGPoint) -> UInt64? = { _ in nil }
}

struct LyricLayers: UIViewRepresentable {
    let setting: LyricSetting
    let stage: LyricStage
    let beat: Int
    let hits: LyricHits
    let seek: (UInt64) -> Void
    let offersFollow: (Bool) -> Void

    func makeUIView(context: Context) -> LyricLayerView {
        LyricLayerView(stage: stage)
    }

    func updateUIView(_ view: LyricLayerView, context: Context) {
        view.seek = seek
        view.offersFollow = offersFollow
        hits.moment = { [weak view] point in view?.moment(at: point) }
        view.take(setting)
    }

    static func dismantleUIView(_ view: LyricLayerView, coordinator: ()) {
        view.stop()
    }
}

enum LyricCadence {
    case resting
    case ticking
    case moving

    var range: CAFrameRateRange {
        switch self {
        case .moving: CAFrameRateRange(minimum: 80, maximum: 120, preferred: 120)
        case .ticking, .resting: CAFrameRateRange(minimum: 20, maximum: 20, preferred: 20)
        }
    }
}

final class LyricLayerView: UIView {
    var seek: (UInt64) -> Void = { _ in }
    var offersFollow: (Bool) -> Void = { _ in }

    private let stage: LyricStage
    private let pane = CALayer()
    private let fade = CAGradientLayer()
    private let sheetLayer = CALayer()
    private var setting: LyricSetting?
    private var link: CADisplayLink?
    private var cadence = LyricCadence.resting
    private var layout: LyricLayout?
    private var rows: [Int: LyricRowLayers] = [:]
    private var end: LyricEndLayers?
    private var reported: (generation: Int, layout: Int)?
    private var layouts = 0
    private var offering = false
    private var shown: LyricScene?
    private var lines: [LyricLineElement] = []

    init(stage: LyricStage) {
        self.stage = stage
        super.init(frame: .zero)
        backgroundColor = .clear
        isOpaque = false
        layer.addSublayer(pane)
        pane.addSublayer(sheetLayer)
        pane.mask = fade
        fade.colors = [UIColor.clear.cgColor, UIColor.black.cgColor, UIColor.black.cgColor, UIColor.clear.cgColor]
        registerForTraitChanges([UITraitDisplayScale.self, UITraitLayoutDirection.self]) { (view: LyricLayerView, _: UITraitCollection) in
            view.forgetLayout()
            view.wake()
        }
    }

    required init?(coder: NSCoder) {
        return nil
    }

    func take(_ taken: LyricSetting) {
        if let held = setting, held.sheet != taken.sheet || held.measures != taken.measures {
            forgetLayout()
        }
        setting = taken
        wake()
    }

    func stop() {
        link?.invalidate()
        link = nil
    }

    override func didMoveToWindow() {
        super.didMoveToWindow()
        if window == nil {
            stop()
        } else {
            wake()
        }
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        pane.frame = bounds
        fade.frame = bounds
        let edge = bounds.height > 0 ? min((setting?.measures.dissolve ?? 0) / bounds.height, 0.25) : 0
        fade.locations = [0, NSNumber(value: Double(edge)), NSNumber(value: Double(1 - edge)), 1]
        CATransaction.commit()
        if let layout, layout.width != bounds.width || layout.paneHeight != bounds.height { forgetLayout() }
        wake()
    }

    private func forgetLayout() {
        layout = nil
        reported = nil
        for row in rows.values { row.container.removeFromSuperlayer() }
        rows = [:]
        end?.container.removeFromSuperlayer()
        end = nil
        lines = []
    }

    private func wake() {
        guard window != nil, setting != nil else { return }
        if link == nil {
            let started = CADisplayLink(target: self, selector: #selector(tick))
            started.preferredFrameRateRange = cadence.range
            started.add(to: .main, forMode: .common)
            link = started
        }
        render(at: Date.now.timeIntervalSinceReferenceDate)
    }

    @objc private func tick(_ link: CADisplayLink) {
        render(at: Date.now.timeIntervalSinceReferenceDate)
    }

    private func render(at now: TimeInterval) {
        guard let setting, bounds.width > 0, bounds.height > 0 else { return }
        let laid = laidOut(setting)
        let scene = stage.scene(
            of: LyricCue(
                sheet: setting.sheet,
                playback: setting.playback,
                native: setting.native,
                pane: bounds.size,
                now: now,
                measures: setting.measures,
                inks: setting.inks,
                still: setting.still,
                scale: traitCollection.displayScale,
                awake: setting.awake,
                animating: cadence == .moving
            )
        )
        report(laid, generation: scene.generation)
        apply(scene, laid)
        shown = scene
        pace(setting, at: now)

        if scene.offersFollow != offering {
            offering = scene.offersFollow
            let offered = offering
            let tell = offersFollow
            DispatchQueue.main.async { tell(offered) }
        }
    }

    private func pace(_ setting: LyricSetting, at now: TimeInterval) {
        let wanted: LyricCadence
        if setting.awake, setting.sweeps || stage.isMoving(at: now) {
            wanted = .moving
        } else if setting.ticks {
            wanted = .ticking
        } else {
            wanted = .resting
        }
        if wanted != cadence {
            cadence = wanted
            link?.preferredFrameRateRange = wanted.range
        }
        link?.isPaused = wanted == .resting
    }

    private func laidOut(_ setting: LyricSetting) -> LyricLayout {
        if let layout { return layout }
        layouts += 1
        let laid = LyricLayout(
            setting.sheet,
            width: bounds.width,
            paneHeight: bounds.height,
            measures: setting.measures,
            rightToLeft: effectiveUserInterfaceLayoutDirection == .rightToLeft
        )
        layout = laid
        sheetLayer.bounds = CGRect(x: 0, y: 0, width: laid.width, height: laid.height)
        sheetLayer.anchorPoint = .zero
        return laid
    }

    private func report(_ laid: LyricLayout, generation: Int) {
        guard reported?.generation != generation || reported?.layout != layouts else { return }
        reported = (generation, layouts)
        for (index, row) in laid.rows.enumerated() {
            stage.measured(row: index, row.frame, generation: generation)
        }
        if let ending = laid.end {
            stage.measured(row: laid.rows.count, ending.frame, generation: generation)
        }
        stage.measuredSheet(laid.height, generation: generation)
    }

    private func apply(_ scene: LyricScene, _ laid: LyricLayout) {
        let scale = traitCollection.displayScale
        let beyond = max(bounds.height * LyricPace.drawsBeyondPanes, LyricPace.drawsWithin)
        let near = (-scene.offset - beyond)...(-scene.offset + bounds.height + beyond)
        let far = (-scene.offset - 2 * beyond)...(-scene.offset + bounds.height + 2 * beyond)

        CATransaction.begin()
        CATransaction.setDisableActions(true)
        pane.opacity = Float(scene.shown)
        sheetLayer.position = CGPoint(x: 0, y: scene.offset)

        for row in scene.rows where row.id < laid.rows.count {
            let placed = laid.rows[row.id]
            let frame = placed.frame
            let isNear = frame.maxY + row.inset >= near.lowerBound && frame.minY + row.inset <= near.upperBound
            guard isNear, !row.look.blank, let typeset = placed.typeset else {
                if let held = rows[row.id] {
                    held.container.isHidden = true
                    if frame.maxY < far.lowerBound || frame.minY > far.upperBound { held.forgetGlyphs() }
                }
                continue
            }
            let layers = rows[row.id] ?? made(placed, at: row.id)
            layers.show(row, typeset: typeset, placed: placed, scale: scale, measures: laid.measures)
        }

        if let ending = scene.end, let placed = laid.end {
            let layers = end ?? madeEnd(placed)
            layers.show(ending, scale: scale)
        } else {
            end?.container.isHidden = true
        }
        CATransaction.commit()

        if UIAccessibility.isVoiceOverRunning { describe(scene, laid) }
    }

    private func made(_ placed: LyricLayout.Row, at index: Int) -> LyricRowLayers {
        let layers = LyricRowLayers(placed)
        sheetLayer.addSublayer(layers.container)
        rows[index] = layers
        return layers
    }

    private func madeEnd(_ placed: LyricLayout.End) -> LyricEndLayers {
        let layers = LyricEndLayers(placed)
        sheetLayer.addSublayer(layers.container)
        end = layers
        return layers
    }

    private func describe(_ scene: LyricScene, _ laid: LyricLayout) {
        if lines.count != scene.rows.count {
            lines = scene.rows.map { _ in LyricLineElement(accessibilityContainer: self) }
        }
        for (index, row) in scene.rows.enumerated() where index < laid.rows.count {
            let element = lines[index]
            let frame = laid.rows[index].frame.offsetBy(dx: 0, dy: scene.offset + row.inset)
            element.accessibilityFrameInContainerSpace = frame
            element.isAccessibilityElement = !row.look.blank
            element.accessibilityLabel = row.look.text
            element.accessibilityValue = row.moment.map(clock)
            element.accessibilityTraits = row.moment == nil ? (row.lit ? .selected : []) : (row.lit ? [.button, .selected] : .button)
            let moment = row.moment
            let seek = seek
            element.activated = { moment.map { seek($0) } != nil }
        }
    }

    override var accessibilityElements: [Any]? {
        get { lines }
        set {}
    }

    override func accessibilityScroll(_ direction: UIAccessibilityScrollDirection) -> Bool {
        let now = Date.now.timeIntervalSinceReferenceDate
        switch direction {
        case .up: stage.turnPage(towards: .top, at: now)
        case .down: stage.turnPage(towards: .bottom, at: now)
        default: return false
        }
        wake()
        UIAccessibility.post(notification: .pageScrolled, argument: nil)
        return true
    }

    func moment(at point: CGPoint) -> UInt64? {
        guard let shown, let layout else { return nil }
        let onSheet = CGPoint(x: point.x, y: point.y - shown.offset)
        let hit = layout.rows.indices.first { index in
            index < shown.rows.count && layout.rows[index].frame.offsetBy(dx: 0, dy: shown.rows[index].inset).contains(onSheet)
        }
        return hit.flatMap { shown.rows[$0].moment }
    }
}

final class LyricLineElement: UIAccessibilityElement {
    var activated: () -> Bool = { false }

    override func accessibilityActivate() -> Bool {
        activated()
    }
}

struct LyricLayout {
    struct Row {
        let frame: CGRect
        let text: CGRect
        let breath: CGRect?
        let side: LyricSide
        let typeset: LyricTypeset?
    }

    struct End {
        let frame: CGRect
        let label: LyricTypeset
        let rule: CGFloat
        let gap: CGFloat
    }

    let width: CGFloat
    let paneHeight: CGFloat
    let measures: LyricMeasures
    let rows: [Row]
    let end: End?
    let height: CGFloat

    init(_ sheet: LyricSheet, width: CGFloat, paneHeight: CGFloat, measures: LyricMeasures, rightToLeft: Bool) {
        let synced = sheet.detail != .unsynced
        let column = min(max(width - 2 * measures.gutter, 0), measures.column)
        let inside = max(column - 2 * measures.pad, 0)
        let textWidth = inside * (sheet.twoVoices ? LyricPace.voiceOfTwoSpans : 1)
        let left = (width - column) / 2
        let edge = synced ? paneHeight / 2 : measures.margin
        let font = UIFont.systemFont(ofSize: measures.words, weight: .bold)

        var rows: [Row] = []
        var y = edge
        for (index, line) in sheet.lines.enumerated() {
            if index > 0 { y += measures.spacing }
            guard !line.blank else {
                let frame = CGRect(x: left, y: y, width: column, height: measures.pause)
                rows.append(Row(frame: frame, text: .zero, breath: nil, side: .centre, typeset: nil))
                y += measures.pause
                continue
            }
            let side: LyricSide = sheet.twoVoices ? (line.voice == 1 ? .trailing : .leading) : .centre
            let typeset = LyricTypeset(
                line.text,
                font: font,
                width: textWidth,
                alignment: side.written(rightToLeft: rightToLeft),
                lineSpacing: measures.lineSpacing
            )
            let breathHeight = line.breathes ? measures.breath : 0
            let textLeft: CGFloat = switch side.placed(rightToLeft: rightToLeft) {
            case .centre: (column - textWidth) / 2
            case .leading: measures.pad
            case .trailing: column - measures.pad - textWidth
            }
            let text = CGRect(x: textLeft, y: breathHeight + measures.padding, width: textWidth, height: typeset.size.height)
            let height = breathHeight + typeset.size.height + 2 * measures.padding
            let breath = line.breathes
                ? CGRect(x: measures.pad, y: 0, width: max(column - 2 * measures.pad, 0), height: measures.breath)
                : nil
            rows.append(Row(frame: CGRect(x: left, y: y, width: column, height: height), text: text, breath: breath, side: side, typeset: typeset))
            y += height
        }

        var end: End?
        if synced {
            if !rows.isEmpty { y += measures.spacing }
            let caption = UIFont.systemFont(ofSize: UIFont.preferredFont(forTextStyle: .caption1).pointSize, weight: .semibold)
            let label = LyricTypeset(
                LyricPace.endOfLyrics,
                font: caption,
                width: .greatestFiniteMagnitude,
                alignment: .left,
                lineSpacing: 0,
                kern: LyricPace.endTracking
            )
            let height = label.size.height + 2 * measures.endPadding
            end = End(frame: CGRect(x: left, y: y, width: column, height: height), label: label, rule: measures.endRule, gap: measures.endGap)
            y += height
        }

        self.width = width
        self.paneHeight = paneHeight
        self.measures = measures
        self.rows = rows
        self.end = end
        height = y + edge
    }
}

extension LyricSide {
    func placed(rightToLeft: Bool) -> LyricSide {
        switch (self, rightToLeft) {
        case (.leading, true): .trailing
        case (.trailing, true): .leading
        default: self
        }
    }

    func written(rightToLeft: Bool) -> NSTextAlignment {
        switch placed(rightToLeft: rightToLeft) {
        case .centre: .center
        case .leading: .left
        case .trailing: .right
        }
    }
}

final class LyricTypeset {
    let size: CGSize

    private let storage: NSTextStorage
    private let manager = NSLayoutManager()
    private let container: NSTextContainer

    init(_ text: String, font: UIFont, width: CGFloat, alignment: NSTextAlignment, lineSpacing: CGFloat, kern: CGFloat = 0) {
        let style = NSMutableParagraphStyle()
        style.alignment = alignment
        style.lineSpacing = lineSpacing
        style.lineBreakMode = .byWordWrapping
        storage = NSTextStorage(
            string: text,
            attributes: [.font: font, .paragraphStyle: style, .foregroundColor: UIColor.black, .kern: kern]
        )
        container = NSTextContainer(size: CGSize(width: width, height: .greatestFiniteMagnitude))
        container.lineFragmentPadding = 0
        manager.addTextContainer(container)
        storage.addLayoutManager(manager)
        manager.ensureLayout(for: container)
        let used = manager.usedRect(for: container)
        size = CGSize(width: width == .greatestFiniteMagnitude ? ceil(used.width) : width, height: ceil(used.height))
    }

    func glyphs(scale: CGFloat) -> CGImage? {
        guard size.width > 0, size.height > 0 else { return nil }
        let format = UIGraphicsImageRendererFormat()
        format.scale = scale
        format.opaque = false
        let drawn = UIGraphicsImageRenderer(size: size, format: format).image { _ in
            manager.drawGlyphs(forGlyphRange: manager.glyphRange(for: container), at: .zero)
        }
        return drawn.cgImage
    }

    func covering(_ range: Range<Int>) -> CGPath {
        let path = CGMutablePath()
        let units = storage.length
        let lower = min(max(range.lowerBound, 0), units)
        let upper = min(max(range.upperBound, lower), units)
        guard upper > lower else { return path }
        let glyphs = manager.glyphRange(forCharacterRange: NSRange(location: lower, length: upper - lower), actualCharacterRange: nil)
        manager.enumerateEnclosingRects(
            forGlyphRange: glyphs,
            withinSelectedGlyphRange: NSRange(location: NSNotFound, length: 0),
            in: container
        ) { rect, _ in
            path.addRect(rect)
        }
        return path
    }
}

final class LyricRowLayers {
    let container = CALayer()

    private let ink = CALayer()
    private let glyphs = CALayer()
    private let sung = CAShapeLayer()
    private let blend = CAShapeLayer()
    private let breath = CALayer()
    private var dots: [CALayer] = []
    private var drawnAt: CGFloat?
    private var swept: LyricSweepCut?

    init(_ placed: LyricLayout.Row) {
        container.frame = placed.frame
        ink.frame = placed.text
        glyphs.frame = ink.bounds
        glyphs.contentsGravity = .resize
        ink.mask = glyphs
        sung.frame = ink.bounds
        blend.frame = ink.bounds
        ink.addSublayer(sung)
        ink.addSublayer(blend)
        container.addSublayer(ink)
        if let area = placed.breath {
            breath.frame = area
            dots = (0..<LyricPace.breathDots).map { _ in CALayer() }
            for dot in dots { breath.addSublayer(dot) }
            container.addSublayer(breath)
        }
    }

    func forgetGlyphs() {
        glyphs.contents = nil
        drawnAt = nil
    }

    func show(_ row: LyricSceneRow, typeset: LyricTypeset, placed: LyricLayout.Row, scale: CGFloat, measures: LyricMeasures) {
        if drawnAt != scale {
            glyphs.contents = typeset.glyphs(scale: scale)
            glyphs.contentsScale = scale
            drawnAt = scale
        }
        container.isHidden = false
        container.opacity = Float(row.look.standing)
        container.setAffineTransform(CGAffineTransform(translationX: 0, y: row.inset))

        switch row.look.words {
        case .plain(let ink):
            self.ink.backgroundColor = ink.cgColor
            if swept != nil {
                sung.path = nil
                blend.path = nil
                swept = nil
            }
        case .swept(let wipe, let unsung, let sungInk):
            let cut = LyricSweepCut(wipe, units: row.look.text.utf16.count)
            ink.backgroundColor = unsung.cgColor
            sung.fillColor = sungInk.cgColor
            blend.fillColor = LyricInks.mixed(unsung, sungInk, by: Double(wipe.blend?.share ?? 0)).cgColor
            if cut != swept {
                sung.path = typeset.covering(0..<cut.sung)
                blend.path = typeset.covering(cut.blendFrom..<cut.blendTo)
                swept = cut
            }
        }

        guard !dots.isEmpty else { return }
        guard let look = row.look.breath else {
            breath.isHidden = true
            return
        }
        breath.isHidden = false
        breath.opacity = Float(look.opacity)
        let widest = measures.dot * (1 + LyricPace.dotSwellsBy)
        let count = CGFloat(dots.count)
        let span = count * widest + (count - 1) * measures.dotGap
        let area = breath.bounds
        let start: CGFloat = switch placed.side {
        case .centre: (area.width - span) / 2
        case .leading: 0
        case .trailing: area.width - span
        }
        for (index, dot) in dots.enumerated() {
            let lit = min(max(look.through * Double(LyricPace.breathDots) - Double(index), 0), 1)
            let side = measures.dot * (1 + LyricPace.dotSwellsBy * look.swell * (0.5 + 0.5 * lit))
            let box = CGRect(x: start + CGFloat(index) * (widest + measures.dotGap), y: (area.height - widest) / 2, width: widest, height: widest)
            dot.frame = CGRect(x: box.midX - side / 2, y: box.midY - side / 2, width: side, height: side)
            dot.cornerRadius = side / 2
            dot.backgroundColor = LyricInks.thinned(look.ink, to: Float(LyricPace.dimDot + (1 - LyricPace.dimDot) * lit)).cgColor
        }
    }
}

struct LyricSweepCut: Equatable {
    let sung: Int
    let blendFrom: Int
    let blendTo: Int

    init(_ wipe: LyricWipe, units: Int) {
        sung = min(Int(wipe.sungUtf16), units)
        blendFrom = min(max(Int(wipe.blend?.startUtf16 ?? wipe.sungUtf16), sung), units)
        blendTo = min(max(Int(wipe.blend?.endUtf16 ?? wipe.sungUtf16), blendFrom), units)
    }
}

final class LyricEndLayers {
    let container = CALayer()

    private let label = CALayer()
    private let glyphs = CALayer()
    private let before = CALayer()
    private let after = CALayer()
    private let typeset: LyricTypeset
    private var drawnAt: CGFloat?

    init(_ placed: LyricLayout.End) {
        typeset = placed.label
        container.frame = placed.frame
        let size = placed.label.size
        let span = size.width + 2 * (placed.rule + placed.gap)
        let left = (placed.frame.width - span) / 2
        let middle = placed.frame.height / 2
        before.frame = CGRect(x: left, y: middle - 0.5, width: placed.rule, height: 1)
        label.frame = CGRect(x: left + placed.rule + placed.gap, y: middle - size.height / 2, width: size.width, height: size.height)
        after.frame = CGRect(x: label.frame.maxX + placed.gap, y: middle - 0.5, width: placed.rule, height: 1)
        glyphs.frame = label.bounds
        glyphs.contentsGravity = .resize
        label.mask = glyphs
        container.addSublayer(before)
        container.addSublayer(label)
        container.addSublayer(after)
    }

    func show(_ end: LyricSceneEnd, scale: CGFloat) {
        if drawnAt != scale {
            glyphs.contents = typeset.glyphs(scale: scale)
            glyphs.contentsScale = scale
            drawnAt = scale
        }
        container.isHidden = false
        container.opacity = Float(end.look.standing)
        container.setAffineTransform(CGAffineTransform(translationX: 0, y: end.inset))
        label.backgroundColor = end.look.muted.cgColor
        before.backgroundColor = end.look.faint.cgColor
        after.backgroundColor = end.look.faint.cgColor
    }
}
