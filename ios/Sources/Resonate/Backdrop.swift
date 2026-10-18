import Combine
import Metal
import os
import ResonateNative
import Synchronization
import SwiftUI
import UIKit

struct BackdropView: View {
    @Environment(Library.self) private var library
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @Environment(\.scenePhase) private var phase
    @Environment(\.colorScheme) private var scheme
    let track: Track?
    let moving: Bool
    let resting: Bool
    @State private var palette: Palette?
    @State private var rendering: BackdropRendering
    @State private var lowPower = ProcessInfo.processInfo.sparesFrames
    @State private var visible = false
    @State private var listening = false

    init(track: Track?, moving: Bool = true, resting: Bool = false) {
        self.track = track
        self.moving = moving
        self.resting = resting
        let recalled = track.flatMap { PaletteMemory.recalled(Self.key(of: $0)) }
        _palette = State(initialValue: recalled ?? (moving ? Self.accentPalette() : nil))
        _rendering = State(initialValue: BackdropPipeline.finished ?? .loading)
    }

    var body: some View {
        ZStack {
            if moving {
                Color(.systemBackground)
            }
            if let palette {
                field(palette)
                    .transition(.opacity)
            }
        }
        .ignoresSafeArea()
        .animation(.easeInOut(duration: 0.4), value: palette == nil)
        .animation(.easeInOut(duration: 0.4), value: rendering.drawsMesh)
        .task { await prepareRendering() }
        .task(id: paletteRequest) { await loadPalette() }
        .onAppear { visible = true }
        .onDisappear {
            visible = false
            listen(false)
        }
        .onChange(of: wantsEnergy) { _, wanted in listen(wanted) }
        .onReceive(ProcessInfo.powerStateChanges) { _ in lowPower = ProcessInfo.processInfo.sparesFrames }
    }

    @ViewBuilder
    private func field(_ palette: Palette) -> some View {
        switch rendering {
        case .metal(let gpu) where moving:
            MeshBackdrop(gpu: gpu, library: library, palette: palette, motion: motion, tone: tone, playing: sounding)
        case .metal(let gpu):
            StillBackdrop(gpu: gpu, palette: palette, tone: tone)
        case .gradient, .loading:
            GradientBackdrop(palette: palette, weight: 1)
        }
    }

    static func prepare(_ track: Track, features: Features) async {
        let key = key(of: track)
        guard PaletteMemory.recalled(key) == nil, let image = await features.artwork(track, bucket: .tile) else { return }
        let sampled = await Backdrop.palette(of: image, accent: accentLinear())
        PaletteMemory.remember(sampled, as: key)
    }

    private var paletteRequest: String {
        guard let track else { return ":\(AppearanceState.shared.accent?.name ?? "")" }
        return "\(Self.key(of: track)):\(library.features.artGeneration(of: track.id)):\(AppearanceState.shared.accent?.name ?? "")"
    }

    private static func key(of track: Track) -> String {
        "\(track.id):\(track.artRevision)"
    }

    private var tone: BackdropTone {
        scheme == .dark ? .dark : .light
    }

    private var motion: BackdropMotion {
        guard phase == .active, !resting else { return .dormant }
        if reduceMotion { return .still }
        return .flowing(framesPerSecond: Self.rate(sounding: sounding, lowPower: lowPower))
    }

    private var sounding: Bool {
        library.transport == .playing
    }

    private static func rate(sounding: Bool, lowPower: Bool) -> Int {
        switch (sounding, lowPower) {
        case (true, false): 120
        case (true, true): 60
        case (false, false): 60
        case (false, true): 30
        }
    }

    private var wantsEnergy: Bool {
        moving && visible && phase == .active && !reduceMotion && rendering.drawsMesh && sounding
    }

    private func listen(_ wanted: Bool) {
        guard wanted != listening else { return }
        listening = wanted
        library.features.hearLoudness(wanted)
    }

    private func prepareRendering() async {
        guard case .loading = rendering else { return }
        rendering = await BackdropPipeline.rendering()
    }

    static func accentLinear() -> SIMD3<Float> {
        let accent = Color.worn.resolve(in: EnvironmentValues())
        return SIMD3<Float>(accent.linearRed, accent.linearGreen, accent.linearBlue)
    }

    static func accentPalette() -> Palette {
        AccentPaletteMemory.palette(around: accentLinear())
    }

    private func loadPalette() async {
        let accentLinear = Self.accentLinear()

        guard let track else {
            let shown = moving ? AccentPaletteMemory.palette(around: accentLinear) : nil
            if palette != shown { palette = shown }
            return
        }
        let found = await library.features.artwork(track, bucket: .tile)
        guard !Task.isCancelled else { return }

        guard let image = found else {
            let around = AccentPaletteMemory.palette(around: accentLinear)
            if palette != around { palette = around }
            return
        }
        let sampled = await Backdrop.palette(of: image, accent: accentLinear)
        guard !Task.isCancelled else { return }

        PaletteMemory.remember(sampled, as: Self.key(of: track))
        if palette != sampled { palette = sampled }
    }
}

struct AppBackdrop: View {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @Environment(\.scenePhase) private var phase
    @Environment(\.colorScheme) private var scheme
    let strength: Float
    let moving: Bool
    let frozen: Bool
    @State private var rendering = BackdropPipeline.finished ?? .loading
    @State private var lowPower = ProcessInfo.processInfo.sparesFrames

    var body: some View {
        let palette = BackdropView.accentPalette()
        let tone = BackdropTone.ambient(scheme, strength: strength)

        ZStack {
            Color(.systemBackground)
            if case .metal(let gpu) = rendering {
                AmbientBackdrop(gpu: gpu, look: AmbientLook(palette: palette, tone: tone, motion: motion))
                    .transition(.opacity)
            } else {
                GradientBackdrop(palette: palette, weight: strength)
            }
        }
        .ignoresSafeArea()
        .animation(.easeInOut(duration: 0.4), value: rendering.drawsMesh)
        .task {
            guard case .loading = rendering else { return }
            rendering = await BackdropPipeline.rendering()
        }
        .onReceive(ProcessInfo.powerStateChanges) { _ in lowPower = ProcessInfo.processInfo.sparesFrames }
    }

    private var motion: AmbientMotion {
        if frozen || phase != .active { return .frozen }
        if reduceMotion || !moving { return .still }
        return .flowing(framesPerSecond: lowPower ? 5 : 10)
    }
}

private struct GradientBackdrop: View {
    let palette: Palette
    let weight: Float

    var body: some View {
        ZStack {
            RadialGradient(
                colors: [Self.color(palette.colours[0]).opacity(0.55 * Double(weight)), .clear],
                center: .topLeading,
                startRadius: 0,
                endRadius: 600
            )
            RadialGradient(
                colors: [Self.color(palette.colours[1]).opacity(0.35 * Double(weight)), .clear],
                center: .bottomTrailing,
                startRadius: 0,
                endRadius: 600
            )
        }
        .animation(.easeInOut(duration: 0.6), value: palette)
    }

    private static func color(_ linear: SIMD3<Float>) -> Color {
        Color(.sRGBLinear, red: Double(linear.x), green: Double(linear.y), blue: Double(linear.z))
    }
}

private enum PaletteMemory {
    private static let most = 64
    private static let kept = Mutex<[String: Palette]>([:])

    static func recalled(_ key: String) -> Palette? {
        kept.withLock { $0[key] }
    }

    static func remember(_ palette: Palette, as key: String) {
        kept.withLock { kept in
            if kept.count >= most { kept.removeAll() }
            kept[key] = palette
        }
    }
}

@MainActor
private enum AccentPaletteMemory {
    private static var kept: (accent: SIMD3<Float>, palette: Palette)?

    static func palette(around accent: SIMD3<Float>) -> Palette {
        if let kept, kept.accent == accent { return kept.palette }
        let palette = Palette.around(accent: accent)
        kept = (accent, palette)
        return palette
    }
}

enum BackdropMotion: Equatable {
    case flowing(framesPerSecond: Int)
    case still
    case dormant
}

enum AmbientMotion: Equatable {
    case flowing(framesPerSecond: Int)
    case still
    case frozen
}

struct BackdropTone: Equatable {
    static let dark = BackdropTone(lightnessFloor: 0.06, lightnessRange: 0.74, chroma: 1, vignette: 0.32)
    static let light = BackdropTone(lightnessFloor: 0.52, lightnessRange: 0.4, chroma: 0.7, vignette: 0.12)

    let lightnessFloor: Float
    let lightnessRange: Float
    let chroma: Float
    let vignette: Float

    static func ambient(_ scheme: ColorScheme, strength: Float) -> BackdropTone {
        let held = min(max(strength, 0), 1)

        if scheme == .dark {
            return BackdropTone(
                lightnessFloor: 0.05,
                lightnessRange: 0.10 + 0.22 * held,
                chroma: 0.5 + 0.5 * held,
                vignette: 0.3
            )
        }
        return BackdropTone(
            lightnessFloor: 0.96 - 0.16 * held,
            lightnessRange: 0.02 + 0.14 * held,
            chroma: 0.3 + 0.5 * held,
            vignette: 0.04
        )
    }
}

struct BackdropGPU: Sendable {
    let device: any MTLDevice
    let queue: any MTLCommandQueue
    let pipeline: any MTLRenderPipelineState
}

enum BackdropRendering: Sendable {
    case loading
    case metal(BackdropGPU)
    case gradient

    var drawsMesh: Bool {
        if case .metal = self { return true }
        return false
    }
}

struct FieldBlend {
    private static let crossfadeSeconds: CFTimeInterval = 1.2

    private(set) var shown: Palette
    private var leaving: Palette
    private(set) var shownPacked: FieldUniforms.Five
    private(set) var leavingPacked: FieldUniforms.Five
    private var began = -CFTimeInterval.infinity

    init(_ palette: Palette) {
        shown = palette
        leaving = palette
        shownPacked = FieldUniforms.packed(palette)
        leavingPacked = shownPacked
    }

    mutating func show(_ palette: Palette, at now: CFTimeInterval) -> Bool {
        guard palette != shown else { return false }
        let reached = Self.eased(progress(at: now))

        leaving = leaving.blended(toward: shown, by: reached)
        shown = palette
        leavingPacked = FieldUniforms.packed(leaving)
        shownPacked = FieldUniforms.packed(shown)
        began = now
        return true
    }

    func progress(at now: CFTimeInterval) -> Float {
        Float(min(max((now - began) / Self.crossfadeSeconds, 0), 1))
    }

    func fading(at now: CFTimeInterval) -> Bool {
        progress(at: now) < 1
    }

    private static func eased(_ progress: Float) -> Float {
        progress * progress * (3 - 2 * progress)
    }
}

struct FieldSize: Equatable, Hashable {
    let width: Int
    let height: Int

    init?(_ size: CGSize) {
        guard size.width >= 1, size.height >= 1 else { return nil }
        width = Int(size.width.rounded(.up))
        height = Int(size.height.rounded(.up))
    }

    var aspect: Float {
        Float(width) / Float(height)
    }
}

enum FieldPicture {
    private static let bytesPerPixel = 4

    @concurrent
    static func rendered(by gpu: BackdropGPU, _ uniforms: FieldUniforms, size: FieldSize) async -> CGImage? {
        guard let encoded = encoded(by: gpu, uniforms, size: size) else { return nil }

        await withCheckedContinuation { continuation in
            encoded.commands.addCompletedHandler(resuming(continuation))
            encoded.commands.commit()
        }
        guard encoded.commands.status == .completed else { return nil }
        return image(of: encoded.texture, size: size)
    }

    static func renderedNow(by gpu: BackdropGPU, _ uniforms: FieldUniforms, size: FieldSize) -> CGImage? {
        guard let encoded = encoded(by: gpu, uniforms, size: size) else { return nil }
        encoded.commands.commit()
        encoded.commands.waitUntilCompleted()
        guard encoded.commands.status == .completed else { return nil }
        return image(of: encoded.texture, size: size)
    }

    private static func resuming(_ continuation: CheckedContinuation<Void, Never>) -> @Sendable (any MTLCommandBuffer) -> Void {
        { _ in continuation.resume() }
    }

    private static func encoded(by gpu: BackdropGPU, _ uniforms: FieldUniforms, size: FieldSize) -> (commands: any MTLCommandBuffer, texture: any MTLTexture)? {
        let descriptor = MTLTextureDescriptor.texture2DDescriptor(
            pixelFormat: BackdropPipeline.pixelFormat,
            width: size.width,
            height: size.height,
            mipmapped: false
        )
        descriptor.usage = .renderTarget
        descriptor.storageMode = .shared
        guard let texture = gpu.device.makeTexture(descriptor: descriptor) else { return nil }

        let pass = MTLRenderPassDescriptor()
        pass.colorAttachments[0].texture = texture
        pass.colorAttachments[0].loadAction = .dontCare
        pass.colorAttachments[0].storeAction = .store
        guard let commands = gpu.queue.makeCommandBuffer(),
            let encoder = commands.makeRenderCommandEncoder(descriptor: pass)
        else { return nil }
        var uniforms = uniforms

        encoder.setRenderPipelineState(gpu.pipeline)
        encoder.setFragmentBytes(&uniforms, length: MemoryLayout<FieldUniforms>.size, index: 0)
        encoder.drawPrimitives(type: .triangle, vertexStart: 0, vertexCount: 3)
        encoder.endEncoding()
        BackdropPipeline.reportFailures(of: commands)
        return (commands, texture)
    }

    private static func image(of texture: any MTLTexture, size: FieldSize) -> CGImage? {
        let bytesPerRow = size.width * bytesPerPixel
        var pixels = Data(count: bytesPerRow * size.height)

        pixels.withUnsafeMutableBytes { bytes in
            guard let base = bytes.baseAddress else { return }
            texture.getBytes(base, bytesPerRow: bytesPerRow, from: MTLRegionMake2D(0, 0, size.width, size.height), mipmapLevel: 0)
        }
        guard let provider = CGDataProvider(data: pixels as CFData), let space = CGColorSpace(name: CGColorSpace.sRGB) else { return nil }
        return CGImage(
            width: size.width,
            height: size.height,
            bitsPerComponent: 8,
            bitsPerPixel: 8 * bytesPerPixel,
            bytesPerRow: bytesPerRow,
            space: space,
            bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.noneSkipFirst.rawValue | CGBitmapInfo.byteOrder32Little.rawValue),
            provider: provider,
            decode: nil,
            shouldInterpolate: true,
            intent: .defaultIntent
        )
    }
}

private struct StillRequest: Equatable {
    let palette: Palette
    let tone: BackdropTone
    let size: FieldSize?
}

@MainActor
private enum StillPictures {
    private static let most = 8
    private static var kept: [(request: StillRequest, image: CGImage)] = []

    static func recalled(_ request: StillRequest) -> CGImage? {
        kept.first { $0.request == request }?.image
    }

    static func remember(_ image: CGImage, for request: StillRequest) {
        kept.removeAll { $0.request == request }
        kept.append((request, image))
        if kept.count > most { kept.removeFirst(kept.count - most) }
    }
}

private struct StillBackdrop: View {
    private struct Shown {
        let image: CGImage
        let order: Int
    }

    let gpu: BackdropGPU
    let palette: Palette
    let tone: BackdropTone
    @State private var size: FieldSize?
    @State private var shown: Shown?

    var body: some View {
        ZStack {
            if let shown {
                Image(decorative: shown.image, scale: 1)
                    .resizable()
                    .id(shown.order)
                    .transition(.opacity)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .onGeometryChange(for: FieldSize?.self) { FieldSize($0.size) } action: { size = $0 }
        .task(id: request) { await draw(request) }
    }

    private var request: StillRequest {
        StillRequest(palette: palette, tone: tone, size: size)
    }

    private func draw(_ request: StillRequest) async {
        guard let size = request.size else { return }
        if let kept = StillPictures.recalled(request) {
            show(kept, fading: shown != nil)
            return
        }
        let uniforms = FieldUniforms.still(palette: request.palette, tone: request.tone, aspect: size.aspect)

        guard let image = await FieldPicture.rendered(by: gpu, uniforms, size: size), !Task.isCancelled else { return }
        StillPictures.remember(image, for: request)
        show(image, fading: true)
    }

    private func show(_ image: CGImage, fading: Bool) {
        let next = Shown(image: image, order: (shown?.order ?? 0) + 1)
        guard fading else {
            shown = next
            return
        }
        withAnimation(.easeInOut(duration: 0.5)) { shown = next }
    }
}

struct AmbientLook: Equatable {
    let palette: Palette
    let tone: BackdropTone
    let motion: AmbientMotion
}

private struct AmbientBackdrop: UIViewRepresentable {
    let gpu: BackdropGPU
    let look: AmbientLook

    func makeUIView(context: Context) -> AmbientLayerView {
        AmbientLayerView(gpu: gpu)
    }

    func updateUIView(_ view: AmbientLayerView, context: Context) {
        view.look = look
    }
}

final class AmbientLayerView: UIView {
    let gpu: BackdropGPU
    private var shown: CGImage?

    var look: AmbientLook? {
        didSet {
            guard window != nil, let look, look != oldValue else { return }
            AmbientField.shared.wear(look, gpu: gpu)
        }
    }

    init(gpu: BackdropGPU) {
        self.gpu = gpu
        super.init(frame: .zero)
        isOpaque = false
        backgroundColor = .clear
        isUserInteractionEnabled = false
        layer.contentsGravity = .resizeAspectFill
    }

    required init?(coder: NSCoder) {
        return nil
    }

    var size: FieldSize? {
        FieldSize(bounds.size)
    }

    override func didMoveToWindow() {
        super.didMoveToWindow()
        guard window != nil else {
            AmbientField.shared.leave(self)
            return
        }
        if let look { AmbientField.shared.wear(look, gpu: gpu) }
        AmbientField.shared.join(self)
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        if window != nil { AmbientField.shared.laidOut(self) }
    }

    func show(_ image: CGImage) {
        guard image !== shown else { return }
        shown = image
        layer.contents = image
    }
}

@MainActor
final class AmbientField: NSObject {
    static let shared = AmbientField()

    private static let speed = 0.18
    private static let loopSeconds = 360.0
    private static let longestStep: CFTimeInterval = 0.25
    private static let fadingFramesPerSecond: Float = 30

    private let views = NSHashTable<AmbientLayerView>.weakObjects()
    private var gpu: BackdropGPU?
    private var blend: FieldBlend?
    private var tone: BackdropTone?
    private var motion = AmbientMotion.frozen
    private var flow: Double = 0
    private var lastStep: CFTimeInterval?
    private var link: CADisplayLink?
    private var drawing = false
    private var owed = false
    private var latest: (image: CGImage, size: FieldSize)?

    func wear(_ look: AmbientLook, gpu: BackdropGPU) {
        let now = CACurrentMediaTime()
        let toneChanged = look.tone != tone
        var changed = toneChanged || look.motion != motion

        self.gpu = gpu
        tone = look.tone
        motion = look.motion
        if var blend {
            changed = blend.show(look.palette, at: now) || changed
            self.blend = blend
        } else {
            blend = FieldBlend(look.palette)
            changed = true
        }
        guard changed else { return }
        if latest == nil {
            drawNow()
        } else if toneChanged {
            draw(at: now)
        } else {
            owed = true
        }
        schedule()
    }

    func join(_ view: AmbientLayerView) {
        views.add(view)
        if let latest, latest.size == wanted {
            view.show(latest.image)
        } else {
            drawNow()
        }
        schedule()
    }

    func leave(_ view: AmbientLayerView) {
        views.remove(view)
        schedule()
    }

    func laidOut(_ view: AmbientLayerView) {
        if let latest, latest.size == wanted {
            view.show(latest.image)
        } else {
            drawNow()
        }
    }

    private var shownOn: [AmbientLayerView] {
        views.allObjects.filter { $0.window != nil }
    }

    private var wanted: FieldSize? {
        shownOn.compactMap(\.size).max { $0.width * $0.height < $1.width * $1.height }
    }

    private var flowingRate: Float? {
        if case .flowing(let framesPerSecond) = motion { return Float(framesPerSecond) }
        return nil
    }

    private func uniforms(at now: CFTimeInterval, size: FieldSize) -> FieldUniforms? {
        guard let blend, let tone else { return nil }
        return FieldUniforms(
            blend: blend,
            time: motion == .still ? FieldUniforms.stillMoment : Float(flow),
            energy: 0,
            progress: blend.progress(at: now),
            aspect: size.aspect,
            tone: tone
        )
    }

    private func drawNow() {
        guard let gpu, let size = wanted, let uniforms = uniforms(at: CACurrentMediaTime(), size: size),
            let image = FieldPicture.renderedNow(by: gpu, uniforms, size: size)
        else { return }
        present(image, size: size)
    }

    private func draw(at now: CFTimeInterval) {
        guard !drawing else {
            owed = true
            return
        }
        guard let gpu, let size = wanted, let uniforms = uniforms(at: now, size: size) else { return }

        owed = false
        drawing = true
        Task {
            let image = await FieldPicture.rendered(by: gpu, uniforms, size: size)
            drawing = false
            if let image { present(image, size: size) }
            schedule()
        }
    }

    private func present(_ image: CGImage, size: FieldSize) {
        latest = (image, size)
        for view in shownOn {
            view.show(image)
        }
    }

    private func schedule() {
        let now = CACurrentMediaTime()
        let fading = blend?.fading(at: now) ?? false
        let running = !shownOn.isEmpty && (flowingRate != nil || fading || owed)

        guard running else {
            link?.invalidate()
            link = nil
            lastStep = nil
            return
        }
        let rate = fading ? Self.fadingFramesPerSecond : flowingRate ?? Self.fadingFramesPerSecond
        let started = link ?? {
            let link = CADisplayLink(target: self, selector: #selector(tick))
            link.add(to: .main, forMode: .common)
            self.link = link
            return link
        }()
        let range = CAFrameRateRange(minimum: rate / 2, maximum: rate, preferred: rate)
        if started.preferredFrameRateRange != range { started.preferredFrameRateRange = range }
    }

    @objc private func tick(_ link: CADisplayLink) {
        let now = link.targetTimestamp

        if flowingRate != nil {
            let step = min(max(now - (lastStep ?? now), 0), Self.longestStep)
            flow = (flow + step * Self.speed).truncatingRemainder(dividingBy: Self.loopSeconds)
        }
        lastStep = now
        draw(at: now)
        schedule()
    }
}

private struct MeshBackdrop: UIViewRepresentable {
    let gpu: BackdropGPU
    let library: Library
    let palette: Palette
    let motion: BackdropMotion
    let tone: BackdropTone
    let playing: Bool

    func makeCoordinator() -> BackdropRenderer {
        BackdropRenderer(gpu: gpu, palette: palette)
    }

    func makeUIView(context: Context) -> BackdropMetalView {
        BackdropMetalView(device: gpu.device, renderer: context.coordinator)
    }

    func updateUIView(_ view: BackdropMetalView, context: Context) {
        let renderer = context.coordinator

        renderer.library = library
        renderer.tone = tone
        renderer.playing = playing
        renderer.still = motion == .still
        renderer.show(palette)
        view.motion = motion
        view.redrawIfStill()
    }

    static func dismantleUIView(_ view: BackdropMetalView, coordinator: BackdropRenderer) {
        view.stop()
    }
}

final class BackdropMetalView: UIView {
    private static let drawingScale: CGFloat = 1
    private static let framesQueuedAhead: Float = 2
    private static let placeholderLingers: TimeInterval = 0.15

    override static var layerClass: AnyClass { CAMetalLayer.self }

    var motion = BackdropMotion.dormant {
        didSet {
            if motion != oldValue { applyMotion() }
        }
    }

    private let renderer: BackdropRenderer
    private var link: CAMetalDisplayLink?
    private var placeholder: CALayer?
    private var drewLiveFrame = false

    init(device: any MTLDevice, renderer: BackdropRenderer) {
        self.renderer = renderer
        super.init(frame: .zero)
        isOpaque = true
        backgroundColor = .clear
        isUserInteractionEnabled = false
        contentScaleFactor = Self.drawingScale
        guard let metal = metalLayer else { return }
        metal.device = device
        metal.pixelFormat = BackdropPipeline.pixelFormat
        metal.framebufferOnly = true
        metal.isOpaque = true
        metal.contentsScale = Self.drawingScale
    }

    required init?(coder: NSCoder) {
        return nil
    }

    private var metalLayer: CAMetalLayer? {
        layer as? CAMetalLayer
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        let size = CGSize(width: bounds.width * Self.drawingScale, height: bounds.height * Self.drawingScale)
        guard let metal = metalLayer, size.width >= 1, size.height >= 1, metal.drawableSize != size else { return }
        metal.drawableSize = size
        placeholder?.frame = bounds
        showPlaceholder()
        redrawIfStill()
    }

    private func showPlaceholder() {
        guard placeholder == nil, !drewLiveFrame, window != nil, UIApplication.shared.applicationState == .active,
            let size = FieldSize(bounds.size),
            let image = renderer.placeholder(aspect: size.aspect, at: CACurrentMediaTime())
        else { return }
        let layer = CALayer()
        layer.frame = bounds
        layer.contents = image
        layer.contentsGravity = .resize
        self.layer.addSublayer(layer)
        placeholder = layer
    }

    private func keepPlaceholder() {
        guard let size = FieldSize(bounds.size) else { return }
        renderer.keepPlaceholder(aspect: size.aspect)
    }

    private func retirePlaceholder() {
        guard let held = placeholder else { return }
        placeholder = nil
        DispatchQueue.main.asyncAfter(deadline: .now() + Self.placeholderLingers) { held.removeFromSuperlayer() }
    }

    override func didMoveToWindow() {
        super.didMoveToWindow()
        contentScaleFactor = Self.drawingScale
        metalLayer?.contentsScale = Self.drawingScale
        if window != nil {
            drewLiveFrame = false
            showPlaceholder()
        } else {
            keepPlaceholder()
        }
        applyMotion()
    }

    func redrawIfStill() {
        if window != nil, motion == .still { link?.isPaused = false }
    }

    func stop() {
        link?.invalidate()
        link = nil
    }

    private func applyMotion() {
        guard window != nil else {
            stop()
            return
        }
        switch motion {
        case .flowing(let framesPerSecond):
            let rate = Float(framesPerSecond)
            let link = running()
            link?.preferredFrameRateRange = CAFrameRateRange(minimum: rate / 2, maximum: rate, preferred: rate)
            link?.isPaused = false
        case .still:
            running()?.isPaused = false
        case .dormant:
            link?.isPaused = true
        }
    }

    private func running() -> CAMetalDisplayLink? {
        if let link { return link }
        guard let metal = metalLayer else { return nil }
        let started = CAMetalDisplayLink(metalLayer: metal)
        started.preferredFrameLatency = Self.framesQueuedAhead
        started.delegate = self
        started.add(to: .main, forMode: .common)
        link = started
        return started
    }

    private func draw(on drawable: any CAMetalDrawable, presentedAt moment: CFTimeInterval) {
        renderer.draw(on: drawable, at: moment)
        drewLiveFrame = true
        retirePlaceholder()
        if motion == .still { link?.isPaused = true }
    }
}

extension BackdropMetalView: @preconcurrency CAMetalDisplayLinkDelegate {
    func metalDisplayLink(_ link: CAMetalDisplayLink, needsUpdate update: CAMetalDisplayLink.Update) {
        draw(on: update.drawable, presentedAt: update.targetPresentationTimestamp)
    }
}

@MainActor
final class BackdropRenderer {
    private static let attackSeconds: Float = 0.12
    private static let releaseSeconds: Float = 0.6
    private static let speedSettlingSeconds: Float = 1.2
    private static let restingSpeed: Float = 0.18
    private static let energyReach: Float = 0.25
    private static let fullEnergyLoudness: Float = 0.25
    private static let loopSeconds: Double = 360
    private static let longestStep: CFTimeInterval = 1.0 / 15
    private static let placeholderWidth = 96

    weak var library: Library?
    var tone = BackdropTone.dark {
        didSet { if tone != oldValue { forgetPlaceholder() } }
    }
    var playing = false
    var still = false {
        didSet { if still != oldValue { forgetPlaceholder() } }
    }

    private let gpu: BackdropGPU
    private var blend: FieldBlend
    private var flow: Double = 0
    private var speed = BackdropRenderer.restingSpeed
    private var energy: Float = 0
    private var lastFrame: CFTimeInterval?
    private var kept: (aspect: Float, image: CGImage)?
    private var keeping = false
    private var keptLook = 0

    init(gpu: BackdropGPU, palette: Palette) {
        self.gpu = gpu
        blend = FieldBlend(palette)
    }

    func show(_ palette: Palette) {
        if blend.show(palette, at: CACurrentMediaTime()) { forgetPlaceholder() }
    }

    private func forgetPlaceholder() {
        kept = nil
        keptLook += 1
    }

    func draw(on drawable: any CAMetalDrawable, at moment: CFTimeInterval) {
        let step = Float(min(max(moment - (lastFrame ?? moment), 0), Self.longestStep))

        lastFrame = moment
        advance(by: step)

        let pass = MTLRenderPassDescriptor()
        pass.colorAttachments[0].texture = drawable.texture
        pass.colorAttachments[0].loadAction = .dontCare
        pass.colorAttachments[0].storeAction = .store
        guard let commands = gpu.queue.makeCommandBuffer(),
            let encoder = commands.makeRenderCommandEncoder(descriptor: pass)
        else { return }
        let aspect = drawable.texture.height > 0 ? Float(drawable.texture.width) / Float(drawable.texture.height) : 1
        var uniforms = uniforms(at: moment, aspect: aspect)

        encoder.setRenderPipelineState(gpu.pipeline)
        encoder.setFragmentBytes(&uniforms, length: MemoryLayout<FieldUniforms>.size, index: 0)
        encoder.drawPrimitives(type: .triangle, vertexStart: 0, vertexCount: 3)
        encoder.endEncoding()
        BackdropPipeline.reportFailures(of: commands)
        commands.present(drawable)
        commands.commit()
    }

    func placeholder(aspect: Float, at moment: CFTimeInterval) -> CGImage? {
        if let kept, abs(kept.aspect - aspect) < Self.placeholderAspectsAlike { return kept.image }
        guard let size = Self.placeholderSize(aspect: aspect) else { return nil }
        return FieldPicture.renderedNow(by: gpu, uniforms(at: moment, aspect: size.aspect), size: size)
    }

    func keepPlaceholder(aspect: Float) {
        let now = CACurrentMediaTime()
        guard !keeping, !blend.fading(at: now), let size = Self.placeholderSize(aspect: aspect) else { return }
        keeping = true
        let gpu = gpu
        let look = keptLook
        let uniforms = uniforms(at: now, aspect: size.aspect)
        Task { [weak self] in
            let image = await FieldPicture.rendered(by: gpu, uniforms, size: size)
            guard let self else { return }
            keeping = false
            if let image, look == keptLook { kept = (aspect, image) }
        }
    }

    private static let placeholderAspectsAlike: Float = 0.01

    private static func placeholderSize(aspect: Float) -> FieldSize? {
        let height = max(Int((Float(placeholderWidth) / max(aspect, 0.1)).rounded()), 1)
        return FieldSize(CGSize(width: placeholderWidth, height: height))
    }

    private func advance(by step: Float) {
        let heard = playing && !still ? loudness() : 0
        let reaction = heard > energy ? Self.attackSeconds : Self.releaseSeconds
        let wantedSpeed = playing ? 1 + Self.energyReach * energy : Self.restingSpeed

        energy += (heard - energy) * Self.settled(step, within: reaction)
        speed += (wantedSpeed - speed) * Self.settled(step, within: Self.speedSettlingSeconds)
        flow = (flow + Double(step * speed)).truncatingRemainder(dividingBy: Self.loopSeconds)
    }

    private func loudness() -> Float {
        guard let rms = library?.features.heardRms else { return 0 }
        return min(rms / Self.fullEnergyLoudness, 1)
    }

    private func uniforms(at now: CFTimeInterval, aspect: Float) -> FieldUniforms {
        FieldUniforms(
            blend: blend,
            time: still ? FieldUniforms.stillMoment : Float(flow),
            energy: still ? 0 : energy,
            progress: still ? 1 : blend.progress(at: now),
            aspect: aspect,
            tone: tone
        )
    }

    private static func settled(_ step: Float, within seconds: Float) -> Float {
        Float(1 - exp(-Double(step / seconds)))
    }
}

struct FieldUniforms {
    typealias Five = (SIMD4<Float>, SIMD4<Float>, SIMD4<Float>, SIMD4<Float>, SIMD4<Float>)

    var colours: Five
    var previous: Five
    var time: Float
    var energy: Float
    var progress: Float
    var aspect: Float
    var lightnessFloor: Float
    var lightnessRange: Float
    var chroma: Float
    var vignette: Float

    static let stillMoment: Float = 13

    init(blend: FieldBlend, time: Float, energy: Float, progress: Float, aspect: Float, tone: BackdropTone) {
        colours = blend.shownPacked
        previous = blend.leavingPacked
        self.time = time
        self.energy = energy
        self.progress = progress
        self.aspect = aspect
        lightnessFloor = tone.lightnessFloor
        lightnessRange = tone.lightnessRange
        chroma = tone.chroma
        vignette = tone.vignette
    }

    static func still(palette: Palette, tone: BackdropTone, aspect: Float) -> FieldUniforms {
        FieldUniforms(blend: FieldBlend(palette), time: stillMoment, energy: 0, progress: 1, aspect: aspect, tone: tone)
    }

    static func packed(_ palette: Palette) -> Five {
        let packed = zip(palette.colours, palette.weights).map { colour, weight in
            let lab = Oklab.lab(fromLinear: pointwiseMax(colour, .zero))
            return SIMD4<Float>(lab.x, lab.y, lab.z, weight)
        }
        return (packed[0], packed[1], packed[2], packed[3], packed[4])
    }
}

enum BackdropPipeline {
    static let pixelFormat = MTLPixelFormat.bgra8Unorm
    private static let shaderUniformBytes = 192
    private static let log = Logger(subsystem: "org.resonate.ios", category: "backdrop")
    private static let settledRendering = Mutex<BackdropRendering?>(nil)
    private static let frameFailureLogged = Atomic<Bool>(false)
    private static let compilation = Task.detached(priority: .utility) { settle(compiled()) }

    static func prepare() {
        _ = compilation
    }

    static var finished: BackdropRendering? {
        settledRendering.withLock { $0 }
    }

    static func rendering() async -> BackdropRendering {
        await compilation.value
    }

    static func reportFailures(of commands: any MTLCommandBuffer) {
        commands.addCompletedHandler(frameFailureHandler())
    }

    private static func frameFailureHandler() -> @Sendable (any MTLCommandBuffer) -> Void {
        { finished in
            guard let error = finished.error, !frameFailureLogged.exchange(true, ordering: .relaxed) else { return }
            logLines(of: "Backdrop frame failed", String(describing: error))
        }
    }

    private static func settle(_ gpu: BackdropGPU?) -> BackdropRendering {
        let rendering = gpu.map(BackdropRendering.metal) ?? .gradient
        settledRendering.withLock { $0 = rendering }
        return rendering
    }

    private static func compiled() -> BackdropGPU? {
        let started = Date()

        guard MemoryLayout<FieldUniforms>.stride == shaderUniformBytes else {
            log.error("Backdrop uniforms are \(MemoryLayout<FieldUniforms>.stride) bytes, the shader reads \(shaderUniformBytes); drawing the gradient")
            return nil
        }
        guard let device = MTLCreateSystemDefaultDevice() else {
            log.error("No Metal device; drawing the gradient backdrop")
            return nil
        }
        guard let queue = device.makeCommandQueue() else {
            log.error("No Metal command queue; drawing the gradient backdrop")
            return nil
        }
        do {
            let library = try device.makeLibrary(source: BackdropShader.source, options: nil)
            guard let vertex = library.makeFunction(name: BackdropShader.vertexName),
                let fragment = library.makeFunction(name: BackdropShader.fragmentName)
            else {
                log.error("Backdrop shader functions are missing; drawing the gradient backdrop")
                return nil
            }
            let descriptor = MTLRenderPipelineDescriptor()
            descriptor.label = "Backdrop"
            descriptor.vertexFunction = vertex
            descriptor.fragmentFunction = fragment
            descriptor.colorAttachments[0].pixelFormat = pixelFormat
            let pipeline = try device.makeRenderPipelineState(descriptor: descriptor)
            let milliseconds = Date().timeIntervalSince(started) * 1000

            log.notice("Backdrop shader ready in \(milliseconds, format: .fixed(precision: 1)) ms")
            return BackdropGPU(device: device, queue: queue, pipeline: pipeline)
        } catch {
            logLines(of: "Backdrop shader failed; drawing the gradient backdrop", String(describing: error))
            return nil
        }
    }

    private static func logLines(of headline: String, _ detail: String) {
        log.error("\(headline, privacy: .public)")
        for line in detail.split(separator: "\n") {
            log.error("\(String(line), privacy: .public)")
        }
    }
}

enum BackdropShader {
    static let vertexName = "backdropVertex"
    static let fragmentName = "backdropFragment"
    static let source = """
        #include <metal_stdlib>
        using namespace metal;

        constant float tau = 6.28318530718;
        constant int blobCount = 5;
        constant float2 blobPeriods[5] = {
            float2(36.0, 45.0), float2(40.0, 24.0), float2(30.0, 40.0), float2(45.0, 36.0), float2(24.0, 30.0)
        };
        constant float2 blobAnchors[5] = {
            float2(0.28, 0.22), float2(0.74, 0.34), float2(0.30, 0.68), float2(0.70, 0.82), float2(0.52, 0.50)
        };
        constant float2 blobPhases[5] = {
            float2(0.0, 1.3), float2(2.1, 0.4), float2(4.2, 3.3), float2(1.1, 5.0), float2(3.6, 2.2)
        };
        constant float2 blobReach = float2(0.26, 0.22);
        constant float blobBaseRadius = 0.38;
        constant float blobWeightRadius = 0.30;
        constant float blobBaseInfluence = 0.35;
        constant float groundInfluence = 0.03;
        constant float chromaKeeping = 0.6;
        constant float chromaRestoredAtMost = 2.5;
        constant float energyGrowth = 0.25;
        constant float broadWarp = 0.42;
        constant float fineWarp = 0.14;
        constant float contrast = 0.35;
        constant float vignetteDesaturation = 0.15;
        constant float ditherStep = 1.0 / 255.0;

        struct Field {
            float4 colours[5];
            float4 previous[5];
            float time;
            float energy;
            float progress;
            float aspect;
            float lightnessFloor;
            float lightnessRange;
            float chroma;
            float vignette;
        };

        struct Varyings {
            float4 position [[position]];
            float2 uv;
        };

        float hashed(float2 p) {
            float3 q = fract(float3(p.x, p.y, p.x) * 0.1031);
            q += dot(q, q.yzx + 33.33);
            return fract((q.x + q.y) * q.z);
        }

        float valueNoise(float2 p) {
            float2 cell = floor(p);
            float2 within = fract(p);
            float2 eased = within * within * within * (within * (within * 6.0 - 15.0) + 10.0);
            float a = hashed(cell);
            float b = hashed(cell + float2(1.0, 0.0));
            float c = hashed(cell + float2(0.0, 1.0));
            float d = hashed(cell + float2(1.0, 1.0));
            return mix(mix(a, b, eased.x), mix(c, d, eased.x), eased.y);
        }

        float3 linearFromOklab(float3 lab) {
            float3 root = float3(
                dot(lab, float3(1.0, 0.3963377774, 0.2158037573)),
                dot(lab, float3(1.0, -0.1055613458, -0.0638541728)),
                dot(lab, float3(1.0, -0.0894841775, -1.2914855480)));
            float3 lms = root * root * root;
            return float3(
                dot(lms, float3(4.0767416621, -3.3077115913, 0.2309699292)),
                dot(lms, float3(-1.2684380046, 2.6097574011, -0.3413193965)),
                dot(lms, float3(-0.0041960863, -0.7034186147, 1.7076147010)));
        }

        float2 blobCentre(int index, float time, float2 frame) {
            float2 angle = tau * time / blobPeriods[index] + blobPhases[index];
            float2 wander = float2(sin(angle.x), cos(angle.y)) * blobReach;
            return (blobAnchors[index] + wander - 0.5) * frame;
        }

        float3 fieldOf(constant float4 *labs, float2 place, float time, float energy, float2 frame) {
            float growth = 1.0 + energyGrowth * energy;
            float3 blended = float3(0.0);
            float3 ground = float3(0.0);
            float blendedChroma = 0.0;
            float groundChroma = 0.0;
            float total = 0.0;
            float weights = 0.0;
            for (int index = 0; index < blobCount; index++) {
                float4 colour = labs[index];
                float3 lab = colour.xyz;
                float chroma = length(lab.yz);
                float2 offset = place - blobCentre(index, time, frame);
                float radius = (blobBaseRadius + blobWeightRadius * sqrt(colour.w)) * growth;
                float influence = (blobBaseInfluence + colour.w) * exp(-dot(offset, offset) / (radius * radius));
                blended += influence * lab;
                blendedChroma += influence * chroma;
                total += influence;
                ground += colour.w * lab;
                groundChroma += colour.w * chroma;
                weights += colour.w;
            }
            float share = max(total + groundInfluence * weights, 0.0001);
            float3 mixed = (blended + groundInfluence * ground) / share;
            float meanChroma = (blendedChroma + groundInfluence * groundChroma) / share;
            float mixedChroma = length(mixed.yz);
            float wantedChroma = mix(mixedChroma, meanChroma, chromaKeeping);
            float restored = min(wantedChroma / max(mixedChroma, 0.02), chromaRestoredAtMost);
            return float3(mixed.x, mixed.yz * restored);
        }

        float2 warped(float2 place, float time, float energy) {
            float2 drift = float2(cos(tau * time / 120.0), sin(tau * time / 120.0)) * 1.6;
            float2 eddy = float2(sin(tau * time / 90.0), cos(tau * time / 90.0)) * 2.4;
            float2 broad = place * 1.25 + drift;
            float2 fine = place * 2.7 - eddy;
            float2 first = float2(valueNoise(broad), valueNoise(broad + float2(5.2, 1.3))) - 0.5;
            float2 second = float2(valueNoise(fine + float2(8.3, 2.8)), valueNoise(fine + float2(1.7, 9.2))) - 0.5;
            return place + (first * broadWarp + second * fineWarp) * (1.0 + energyGrowth * energy);
        }

        float3 shaped(float3 lab, float2 uv, constant Field &field) {
            float lightness = saturate(lab.x);
            float curved = mix(lightness, lightness * lightness * (3.0 - 2.0 * lightness), contrast);
            float edge = smoothstep(0.30, 0.95, length((uv - 0.5) * 2.0) * 0.7071);
            float darkened = (field.lightnessFloor + field.lightnessRange * curved) * (1.0 - field.vignette * edge);
            float kept = sqrt(saturate(darkened / max(lightness, 0.05)));
            float chroma = field.chroma * kept * (1.0 - vignetteDesaturation * edge);
            return float3(darkened, lab.yz * chroma);
        }

        float3 srgbEncoded(float3 rgb) {
            float3 low = rgb * 12.92;
            float3 high = 1.055 * powr(max(rgb, float3(0.0031308)), float3(1.0 / 2.4)) - 0.055;
            return select(high, low, rgb <= float3(0.0031308));
        }

        vertex Varyings backdropVertex(uint vertexID [[vertex_id]]) {
            float2 corner = float2(float((vertexID << 1) & 2), float(vertexID & 2));
            Varyings out;
            out.position = float4(corner * 2.0 - 1.0, 0.0, 1.0);
            out.uv = float2(corner.x, 1.0 - corner.y);
            return out;
        }

        fragment float4 backdropFragment(Varyings in [[stage_in]], constant Field &field [[buffer(0)]]) {
            float aspect = max(field.aspect, 0.01);
            float2 frame = float2(max(aspect, 1.0), max(1.0 / aspect, 1.0));
            float2 place = warped((in.uv - 0.5) * frame, field.time, field.energy);
            float3 lab = fieldOf(field.colours, place, field.time, field.energy, frame);
            if (field.progress < 1.0) {
                float3 before = fieldOf(field.previous, place, field.time, field.energy, frame);
                lab = mix(before, lab, smoothstep(0.0, 1.0, field.progress));
            }
            float3 rgb = saturate(linearFromOklab(shaped(lab, in.uv, field)));
            float dither = (hashed(in.position.xy) + hashed(in.position.xy + float2(17.0, 59.0)) - 1.0) * ditherStep;
            return float4(srgbEncoded(rgb) + dither, 1.0);
        }
        """
}

extension ProcessInfo {
    var sparesFrames: Bool {
        isLowPowerModeEnabled || thermalState == .serious || thermalState == .critical
    }

    static var powerStateChanges: some Publisher<Notification, Never> {
        NotificationCenter.default
            .publisher(for: .NSProcessInfoPowerStateDidChange)
            .merge(with: NotificationCenter.default.publisher(for: ProcessInfo.thermalStateDidChangeNotification))
            .receive(on: DispatchQueue.main)
    }
}
