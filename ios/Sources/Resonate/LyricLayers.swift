import Observation
import ResonateNative
import SwiftUI
import UIKit

struct LyricSetting {
    let sheet: LyricSheet
    let heard: LyricHeard?
    let clock: PlayClock
    let native: MobilePlayer?
    let measures: LyricMeasures
    let inks: LyricInks
    let still: Bool
    let awake: Bool
    let sweeps: Bool
    let ticks: Bool

    @MainActor var playback: LyricPlayback? {
        heard.map { LyricPlayback(track: $0.track, positionMs: clock.positionMs, playing: $0.playing) }
    }
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

    private static let presentsWithin: TimeInterval = 0.05

    private let stage: LyricStage
    private let pane = CALayer()
    private let fade = CAGradientLayer()
    private let sheetLayer = CALayer()
    private var setting: LyricSetting?
    private var link: CADisplayLink?
    private var cadence = LyricCadence.resting
    private var shapes: LyricShapes?
    private var shaping: LyricShapeKey?
    private var layout: LyricLayout?
    private var rows: [Int: LyricRowLayers] = [:]
    private var end: LyricEndLayers?
    private var reported: (generation: Int, layout: Int)?
    private var layouts = 0
    private var offering = false
    private var shown: LyricScene?
    private var lines: [LyricLineElement] = []
    private var watching = false
    private var drewFor: TimeInterval = 0

    init(stage: LyricStage) {
        self.stage = stage
        super.init(frame: .zero)
        backgroundColor = .clear
        isOpaque = false
        sheetLayer.anchorPoint = .zero
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
        watchClock()
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
        if let shapes, shapes.key.width != bounds.width {
            forgetLayout()
        } else if let layout, layout.paneHeight != bounds.height {
            self.layout = nil
            reported = nil
        }
        wake()
    }

    private func watchClock() {
        guard !watching, let clock = setting?.clock else { return }
        watching = true
        withObservationTracking {
            _ = clock.positionMs
        } onChange: { [weak self] in
            let view = self
            Task { @MainActor in view?.clockMoved() }
        }
    }

    private func clockMoved() {
        watching = false
        watchClock()
        wake()
    }

    private func forgetLayout() {
        shapes = nil
        layout = nil
        reported = nil
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        for row in rows.values { row.container.removeFromSuperlayer() }
        end?.container.removeFromSuperlayer()
        CATransaction.commit()
        rows = [:]
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
        render(at: presented())
    }

    @objc private func tick(_ link: CADisplayLink) {
        render(at: presented())
    }

    private func presented() -> TimeInterval {
        let now = Date.now.timeIntervalSinceReferenceDate
        guard let link, !link.isPaused else { return now }
        return now + min(max(link.targetTimestamp - CACurrentMediaTime(), 0), Self.presentsWithin)
    }

    private func render(at asked: TimeInterval) {
        guard let setting, bounds.width > 0, bounds.height > 0, let laid = laidOut(setting) else { return }
        let now = max(asked, drewFor)
        drewFor = now
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
        if wanted == .resting {
            for row in rows.values { row.rest() }
        }
    }

    private func laidOut(_ setting: LyricSetting) -> LyricLayout? {
        if let layout { return layout }
        let key = LyricShapeKey(
            sheet: setting.sheet,
            width: bounds.width,
            measures: setting.measures,
            rightToLeft: effectiveUserInterfaceLayoutDirection == .rightToLeft,
            caption: UIFont.preferredFont(forTextStyle: .caption1).pointSize
        )
        guard let shaped = shapes.flatMap({ $0.key == key ? $0 : nil }) ?? LyricShapesKept.find(key) else {
            shape(key)
            return nil
        }
        shapes = shaped
        layouts += 1
        let laid = LyricLayout(shaped, paneHeight: bounds.height)
        layout = laid
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        sheetLayer.bounds = CGRect(x: 0, y: 0, width: key.width, height: laid.height)
        CATransaction.commit()
        return laid
    }

    private func shape(_ key: LyricShapeKey) {
        guard shaping != key else { return }
        shaping = key
        Task { [weak self] in
            let shaped = await LyricGlyphs.shaped(key)
            LyricShapesKept.keep(shaped)
            guard let self, self.shaping == key else { return }
            self.shaping = nil
            self.wake()
        }
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
        let middle = -scene.offset + bounds.height / 2
        var owed: [(away: CGFloat, layers: LyricRowLayers)] = []

        CATransaction.begin()
        CATransaction.setDisableActions(true)
        let shown = Float(scene.shown)
        if pane.opacity != shown { pane.opacity = shown }
        let position = CGPoint(x: 0, y: scene.offset)
        if sheetLayer.position != position { sheetLayer.position = position }

        for row in scene.rows where row.id < laid.rows.count {
            let placed = laid.rows[row.id]
            let frame = placed.frame
            let isNear = frame.maxY + row.inset >= near.lowerBound && frame.minY + row.inset <= near.upperBound
            guard isNear, !row.look.blank, let typeset = placed.typeset else {
                if let held = rows[row.id] {
                    held.hide()
                    if frame.maxY < far.lowerBound || frame.minY > far.upperBound { held.forgetGlyphs() }
                }
                continue
            }
            let layers = rows[row.id] ?? made(placed, at: row.id)
            if layers.show(row, typeset: typeset, placed: placed, scale: scale, measures: laid.measures) {
                owed.append((abs(frame.midY + row.inset - middle), layers))
            }
        }

        if let ending = scene.end, let placed = laid.end {
            let layers = end ?? madeEnd(placed)
            layers.show(ending, placed: placed, scale: scale)
        } else {
            end?.hide()
        }
        CATransaction.commit()

        for owing in owed.sorted(by: { $0.away < $1.away }) { owing.layers.paint(at: scale) }

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

struct LyricShapeKey: Equatable, Sendable {
    let sheet: LyricSheet
    let width: CGFloat
    let measures: LyricMeasures
    let rightToLeft: Bool
    let caption: CGFloat
}

struct LyricShapes: Sendable {
    struct Row: Sendable {
        let height: CGFloat
        let text: CGRect
        let breath: CGRect?
        let side: LyricSide
        let typeset: LyricTypeset?
    }

    struct End: Sendable {
        let height: CGFloat
        let label: LyricTypeset
    }

    let key: LyricShapeKey
    let left: CGFloat
    let column: CGFloat
    let rows: [Row]
    let end: End?

    init(_ key: LyricShapeKey) {
        let sheet = key.sheet
        let measures = key.measures
        let column = min(max(key.width - 2 * measures.gutter, 0), measures.column)
        let inside = max(column - 2 * measures.pad, 0)
        let textWidth = inside * (sheet.twoVoices ? LyricPace.voiceOfTwoSpans : 1)
        let font = UIFont.systemFont(ofSize: measures.words, weight: .bold)

        rows = sheet.lines.map { line in
            guard !line.blank else {
                return Row(height: measures.pause, text: .zero, breath: nil, side: .centre, typeset: nil)
            }
            let side: LyricSide = sheet.twoVoices ? (line.voice == 1 ? .trailing : .leading) : .centre
            let typeset = LyricTypeset(
                line.text,
                font: font,
                width: textWidth,
                alignment: side.written(rightToLeft: key.rightToLeft),
                lineSpacing: measures.lineSpacing
            )
            let breathHeight = line.breathes ? measures.breath : 0
            let textLeft: CGFloat = switch side.placed(rightToLeft: key.rightToLeft) {
            case .centre: (column - textWidth) / 2
            case .leading: measures.pad
            case .trailing: column - measures.pad - textWidth
            }
            let text = CGRect(x: textLeft, y: breathHeight + measures.padding, width: textWidth, height: typeset.size.height)
            let breath = line.breathes
                ? CGRect(x: measures.pad, y: 0, width: max(column - 2 * measures.pad, 0), height: measures.breath)
                : nil
            return Row(height: breathHeight + typeset.size.height + 2 * measures.padding, text: text, breath: breath, side: side, typeset: typeset)
        }

        if sheet.detail != .unsynced {
            let label = LyricTypeset(
                LyricPace.endOfLyrics,
                font: UIFont.systemFont(ofSize: key.caption, weight: .semibold),
                width: .greatestFiniteMagnitude,
                alignment: .left,
                lineSpacing: 0,
                kern: LyricPace.endTracking
            )
            end = End(height: label.size.height + 2 * measures.endPadding, label: label)
        } else {
            end = nil
        }

        self.key = key
        left = (key.width - column) / 2
        self.column = column
    }
}

@MainActor
enum LyricShapesKept {
    private static let most = 4
    private static var kept: [LyricShapes] = []

    static func find(_ key: LyricShapeKey) -> LyricShapes? {
        kept.last { $0.key == key }
    }

    static func keep(_ shapes: LyricShapes) {
        kept.removeAll { $0.key == shapes.key }
        kept.append(shapes)
        if kept.count > most { kept.removeFirst(kept.count - most) }
    }
}

enum LyricGlyphs {
    private static let queue = DispatchQueue(label: "org.resonate.lyric-glyphs", qos: .userInitiated)

    static func shaped(_ key: LyricShapeKey) async -> LyricShapes {
        await withCheckedContinuation { continuation in
            queue.async { continuation.resume(returning: LyricShapes(key)) }
        }
    }

    static func drawn(_ typeset: LyricTypeset, scale: CGFloat) async -> CGImage? {
        await withCheckedContinuation { continuation in
            queue.async { continuation.resume(returning: typeset.glyphs(scale: scale)) }
        }
    }

    static func tinted(_ typeset: LyricTypeset, ink: CGColor, scale: CGFloat) async -> CGImage? {
        await withCheckedContinuation { continuation in
            queue.async { continuation.resume(returning: typeset.tinted(ink, scale: scale)) }
        }
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

    let paneHeight: CGFloat
    let measures: LyricMeasures
    let rows: [Row]
    let end: End?
    let height: CGFloat

    init(_ shapes: LyricShapes, paneHeight: CGFloat) {
        let measures = shapes.key.measures
        let edge = shapes.key.sheet.detail != .unsynced ? paneHeight / 2 : measures.margin

        var rows: [Row] = []
        var y = edge
        for (index, shape) in shapes.rows.enumerated() {
            if index > 0 { y += measures.spacing }
            let frame = CGRect(x: shapes.left, y: y, width: shapes.column, height: shape.height)
            rows.append(Row(frame: frame, text: shape.text, breath: shape.breath, side: shape.side, typeset: shape.typeset))
            y += shape.height
        }

        var end: End?
        if let ending = shapes.end {
            if !rows.isEmpty { y += measures.spacing }
            let frame = CGRect(x: shapes.left, y: y, width: shapes.column, height: ending.height)
            end = End(frame: frame, label: ending.label, rule: measures.endRule, gap: measures.endGap)
            y += ending.height
        }

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

final class LyricTypeset: @unchecked Sendable {
    let size: CGSize

    private let lock = NSLock()
    private let text: String
    private let attributes: [NSAttributedString.Key: Any]
    private let width: CGFloat
    private let laid: LyricTypesetting

    init(_ text: String, font: UIFont, width: CGFloat, alignment: NSTextAlignment, lineSpacing: CGFloat, kern: CGFloat = 0) {
        let style = NSMutableParagraphStyle()
        style.alignment = alignment
        style.lineSpacing = lineSpacing
        style.lineBreakMode = .byWordWrapping
        self.text = text
        attributes = [.font: font, .paragraphStyle: style, .kern: kern]
        self.width = width
        laid = LyricTypesetting(text, attributes: attributes, ink: .black, width: width)
        let used = laid.manager.usedRect(for: laid.container)
        size = CGSize(width: width == .greatestFiniteMagnitude ? ceil(used.width) : width, height: ceil(used.height))
    }

    func glyphs(scale: CGFloat) -> CGImage? {
        lock.withLock {
            let wide = Int((size.width * scale).rounded(.up))
            let high = Int((size.height * scale).rounded(.up))
            guard wide > 0, high > 0 else { return nil }
            guard let context = CGContext(
                data: nil,
                width: wide,
                height: high,
                bitsPerComponent: 8,
                bytesPerRow: 0,
                space: CGColorSpaceCreateDeviceGray(),
                bitmapInfo: CGImageAlphaInfo.alphaOnly.rawValue
            ) else {
                return laid.drawn(size: size, scale: scale)
            }
            context.translateBy(x: 0, y: CGFloat(high))
            context.scaleBy(x: scale, y: -scale)
            UIGraphicsPushContext(context)
            laid.draw()
            UIGraphicsPopContext()
            return context.makeImage()
        }
    }

    func tinted(_ ink: CGColor, scale: CGFloat) -> CGImage? {
        guard size.width > 0, size.height > 0 else { return nil }
        return LyricTypesetting(text, attributes: attributes, ink: UIColor(cgColor: ink), width: width).drawn(size: size, scale: scale)
    }

    func covering(_ range: Range<Int>) -> CGPath {
        lock.withLock { laid.covering(range) }
    }
}

private final class LyricTypesetting {
    let storage: NSTextStorage
    let manager = NSLayoutManager()
    let container: NSTextContainer

    init(_ text: String, attributes: [NSAttributedString.Key: Any], ink: UIColor, width: CGFloat) {
        var inked = attributes
        inked[.foregroundColor] = ink
        storage = NSTextStorage(string: text, attributes: inked)
        container = NSTextContainer(size: CGSize(width: width, height: .greatestFiniteMagnitude))
        container.lineFragmentPadding = 0
        manager.addTextContainer(container)
        storage.addLayoutManager(manager)
        manager.ensureLayout(for: container)
    }

    func draw() {
        manager.drawGlyphs(forGlyphRange: manager.glyphRange(for: container), at: .zero)
    }

    func drawn(size: CGSize, scale: CGFloat) -> CGImage? {
        guard size.width > 0, size.height > 0 else { return nil }
        let format = UIGraphicsImageRendererFormat()
        format.scale = scale
        format.opaque = false
        format.preferredRange = .standard
        return UIGraphicsImageRenderer(size: size, format: format).image { _ in draw() }.cgImage
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

struct LyricTint {
    let ink: Color.Resolved
    let scale: CGFloat
    let image: CGImage
}

@MainActor
final class LyricRowLayers {
    let container = CALayer()

    private let ink = CALayer()
    private let glyphs = CALayer()
    private let sung = CAShapeLayer()
    private let blend = CAShapeLayer()
    private let breath = CALayer()
    private var dots: [CALayer] = []
    private var typeset: LyricTypeset?
    private var drawnAt: CGFloat?
    private var painting: CGFloat?
    private var tint: LyricTint?
    private var tinting: Color.Resolved?
    private var worn: CGImage?
    private var plain: Color.Resolved?
    private var background: Color.Resolved?
    private var sungFill: Color.Resolved?
    private var blendFill: Color.Resolved?
    private var swept: LyricSweepCut?
    private var standing: Float?
    private var inset: CGFloat?

    init(_ placed: LyricLayout.Row) {
        container.frame = placed.frame
        container.allowsGroupOpacity = false
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

    func hide() {
        if !container.isHidden { container.isHidden = true }
    }

    func forgetGlyphs() {
        glyphs.contents = nil
        drawnAt = nil
        painting = nil
        tint = nil
        tinting = nil
        unwear()
    }

    func paint(at scale: CGFloat) {
        guard let typeset, drawnAt != scale, painting != scale else { return }
        painting = scale
        Task { [weak self] in
            let drawn = await LyricGlyphs.drawn(typeset, scale: scale)
            guard let self, self.painting == scale else { return }
            self.painting = nil
            self.drawnAt = scale
            self.tint = nil
            CATransaction.begin()
            CATransaction.setDisableActions(true)
            self.unwear()
            self.glyphs.contents = drawn
            self.glyphs.contentsScale = scale
            CATransaction.commit()
            if let plain = self.plain { self.tint(plain, at: scale) }
        }
    }

    func rest() {
        guard let plain, let drawnAt else { return }
        tint(plain, at: drawnAt)
    }

    func show(_ row: LyricSceneRow, typeset: LyricTypeset, placed: LyricLayout.Row, scale: CGFloat, measures: LyricMeasures) -> Bool {
        self.typeset = typeset
        if container.isHidden { container.isHidden = false }
        let middle = CGPoint(x: placed.frame.midX, y: placed.frame.midY)
        if container.position != middle { container.position = middle }
        let standing = Float(row.look.standing)
        if self.standing != standing {
            container.opacity = standing
            self.standing = standing
        }
        if inset != row.inset {
            container.setAffineTransform(CGAffineTransform(translationX: 0, y: row.inset))
            inset = row.inset
        }

        switch row.look.words {
        case .plain(let colour):
            if swept != nil {
                sung.path = nil
                blend.path = nil
                swept = nil
            }
            if container.allowsGroupOpacity { container.allowsGroupOpacity = false }
            if let tint, tint.ink == colour, tint.scale == drawnAt {
                wear(tint)
            } else {
                masked(colour)
                if plain == colour, let drawnAt { self.tint(colour, at: drawnAt) }
            }
            plain = colour
        case .swept(let wipe, let unsung, let sungInk):
            plain = nil
            if !container.allowsGroupOpacity { container.allowsGroupOpacity = true }
            masked(unsung)
            let cut = LyricSweepCut(wipe, units: row.look.text.utf16.count)
            if sungFill != sungInk {
                sung.fillColor = sungInk.cgColor
                sungFill = sungInk
            }
            let blending = LyricInks.mixed(unsung, sungInk, by: Double(wipe.blend?.share ?? 0))
            if blendFill != blending {
                blend.fillColor = blending.cgColor
                blendFill = blending
            }
            if cut != swept {
                sung.path = typeset.covering(0..<cut.sung)
                blend.path = typeset.covering(cut.blendFrom..<cut.blendTo)
                swept = cut
            }
        }

        breathe(row, placed: placed, measures: measures)
        return drawnAt != scale && painting != scale
    }

    private func breathe(_ row: LyricSceneRow, placed: LyricLayout.Row, measures: LyricMeasures) {
        guard !dots.isEmpty else { return }
        guard let look = row.look.breath else {
            if !breath.isHidden { breath.isHidden = true }
            return
        }
        if breath.isHidden { breath.isHidden = false }
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

    private func tint(_ colour: Color.Resolved, at scale: CGFloat) {
        guard let typeset, tinting != colour, tint?.ink != colour || tint?.scale != scale else { return }
        tinting = colour
        let cgColour = colour.cgColor
        Task { [weak self] in
            let image = await LyricGlyphs.tinted(typeset, ink: cgColour, scale: scale)
            guard let self, self.tinting == colour else { return }
            self.tinting = nil
            guard let image, self.drawnAt == scale else { return }
            let tint = LyricTint(ink: colour, scale: scale, image: image)
            self.tint = tint
            guard self.plain == colour else { return }
            CATransaction.begin()
            CATransaction.setDisableActions(true)
            self.wear(tint)
            CATransaction.commit()
        }
    }

    private func wear(_ tint: LyricTint) {
        guard worn !== tint.image else { return }
        ink.mask = nil
        ink.backgroundColor = nil
        background = nil
        ink.contents = tint.image
        ink.contentsScale = tint.scale
        worn = tint.image
    }

    private func unwear() {
        guard worn != nil else { return }
        ink.contents = nil
        ink.mask = glyphs
        worn = nil
    }

    private func masked(_ colour: Color.Resolved) {
        unwear()
        if background != colour {
            ink.backgroundColor = colour.cgColor
            background = colour
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

@MainActor
final class LyricEndLayers {
    let container = CALayer()

    private let label = CALayer()
    private let glyphs = CALayer()
    private let before = CALayer()
    private let after = CALayer()
    private let typeset: LyricTypeset
    private var drawnAt: CGFloat?
    private var painting: CGFloat?
    private var look: LyricEndLook?
    private var inset: CGFloat?

    init(_ placed: LyricLayout.End) {
        typeset = placed.label
        container.frame = placed.frame
        container.allowsGroupOpacity = false
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

    func hide() {
        if !container.isHidden { container.isHidden = true }
    }

    func show(_ end: LyricSceneEnd, placed: LyricLayout.End, scale: CGFloat) {
        paint(at: scale)
        if container.isHidden { container.isHidden = false }
        let middle = CGPoint(x: placed.frame.midX, y: placed.frame.midY)
        if container.position != middle { container.position = middle }
        if inset != end.inset {
            container.setAffineTransform(CGAffineTransform(translationX: 0, y: end.inset))
            inset = end.inset
        }
        guard look != end.look else { return }
        if look?.standing != end.look.standing { container.opacity = Float(end.look.standing) }
        if look?.muted != end.look.muted { label.backgroundColor = end.look.muted.cgColor }
        if look?.faint != end.look.faint {
            let faint = end.look.faint.cgColor
            before.backgroundColor = faint
            after.backgroundColor = faint
        }
        look = end.look
    }

    private func paint(at scale: CGFloat) {
        guard drawnAt != scale, painting != scale else { return }
        painting = scale
        let typeset = typeset
        Task { [weak self] in
            let drawn = await LyricGlyphs.drawn(typeset, scale: scale)
            guard let self, self.painting == scale else { return }
            self.painting = nil
            self.drawnAt = scale
            CATransaction.begin()
            CATransaction.setDisableActions(true)
            self.glyphs.contents = drawn
            self.glyphs.contentsScale = scale
            CATransaction.commit()
        }
    }
}
