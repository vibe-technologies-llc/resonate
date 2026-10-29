import Combine
import Metal
import os
import ResonateLogic
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
            MeshBackdrop(gpu: gpu, heard: library.features.heardLevel, palette: palette, motion: motion, tone: tone, playing: sounding)
        case .metal(let gpu):
            StillBackdrop(gpu: gpu, palette: palette, tone: tone)
        case .gradient, .loading:
            GradientBackdrop(palette: palette, weight: 1)
        }
    }

    static func prepare(_ track: Track, features: Features) async {
        let key = key(of: track)
        guard PaletteMemory.recalled(key) == nil, let image = await features.artwork(track, bucket: .tile) else { return }
        let sampled = await Backdrop.palette(of: image, accent: accentLinear(shownScheme))
        PaletteMemory.remember(sampled, as: key)
    }

    private var paletteRequest: String {
        let accent = "\(AppearanceState.shared.accent?.name ?? ""):\(scheme == .dark ? "dark" : "light")"
        guard let track else { return ":\(accent)" }
        let generation = track.isServed ? library.features.servedGeneration : library.features.artGeneration(of: track.id)
        return "\(Self.key(of: track)):\(generation):\(accent)"
    }

    private static func key(of track: Track) -> String {
        "\(track.pictureKey):\(track.artRevision)"
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

    static var shownScheme: ColorScheme {
        UITraitCollection.current.userInterfaceStyle == .dark ? .dark : .light
    }

    static func accentLinear(_ scheme: ColorScheme) -> SIMD3<Float> {
        var values = EnvironmentValues()
        values.colorScheme = scheme
        let accent = Color.worn.resolve(in: values)
        return SIMD3<Float>(accent.linearRed, accent.linearGreen, accent.linearBlue)
    }

    static func accentPalette(_ scheme: ColorScheme = shownScheme) -> Palette {
        AccentPaletteMemory.palette(around: accentLinear(scheme))
    }

    private func loadPalette() async {
        let accentLinear = Self.accentLinear(scheme)

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
        let palette = BackdropView.accentPalette(scheme)
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

extension BackdropTone {
    static func ambient(_ scheme: ColorScheme, strength: Float) -> BackdropTone {
        ambient(dark: scheme == .dark, strength: strength)
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

    func stretches(_ held: FieldSize) -> Bool {
        Double(width) > Double(held.width) * Self.stretchedAtMost || Double(height) > Double(held.height) * Self.stretchedAtMost
    }

    private static let stretchedAtMost = 1.25
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

    private override init() {
        super.init()
        NotificationCenter.default.addObserver(
            self,
            selector: #selector(cameForward),
            name: UIApplication.didBecomeActiveNotification,
            object: nil
        )
    }

    @objc private func cameForward() {
        link?.invalidate()
        link = nil
        lastStep = nil
        drawing = false
        owed = true
        schedule()
    }

    func wear(_ look: AmbientLook, gpu: BackdropGPU) {
        let now = CACurrentMediaTime()
        let toneChanged = look.tone != tone
        let motionMoved = look.motion != motion
        let stillOwed = look.motion == .still && motion != .still
        var changed = toneChanged || stillOwed

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
        guard changed || motionMoved else { return }
        guard changed else {
            schedule()
            return
        }
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
        showWhatIsHeld(on: view)
        schedule()
    }

    func leave(_ view: AmbientLayerView) {
        views.remove(view)
        schedule()
    }

    func laidOut(_ view: AmbientLayerView) {
        showWhatIsHeld(on: view)
    }

    private func showWhatIsHeld(on view: AmbientLayerView) {
        guard let latest else {
            drawNow()
            return
        }
        view.show(latest.image)
        if let wanted, wanted.stretches(latest.size) {
            owed = true
            schedule()
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
    let heard: HeardLevel
    let palette: Palette
    let motion: BackdropMotion
    let tone: BackdropTone
    let playing: Bool

    func makeCoordinator() -> BackdropRenderer {
        BackdropRenderer(gpu: gpu, palette: palette, heard: heard)
    }

    func makeUIView(context: Context) -> BackdropMetalView {
        BackdropMetalView(device: gpu.device, renderer: context.coordinator)
    }

    func updateUIView(_ view: BackdropMetalView, context: Context) {
        context.coordinator.wear(tone: tone, playing: playing, still: motion == .still, palette: palette)
        view.motion = motion
        view.redrawIfStill()
    }

    static func dismantleUIView(_ view: BackdropMetalView, coordinator: BackdropRenderer) {
        view.stop()
    }
}

final class HeardLevel: Sendable {
    private let bits = Atomic<UInt32>(0)

    var rms: Float {
        Float(bitPattern: bits.load(ordering: .relaxed))
    }

    func hear(_ rms: Float) {
        bits.store(rms.bitPattern, ordering: .relaxed)
    }
}

final class FieldThread: Thread, @unchecked Sendable {
    static let shared = FieldThread.started()

    private let ready = DispatchSemaphore(value: 0)
    private var loop: CFRunLoop?

    private static func started() -> FieldThread {
        let thread = FieldThread()
        thread.name = "org.resonate.backdrop"
        thread.qualityOfService = .userInteractive
        thread.start()
        thread.ready.wait()
        return thread
    }

    override func main() {
        loop = CFRunLoopGetCurrent()
        RunLoop.current.add(NSMachPort(), forMode: .default)
        ready.signal()
        while !isCancelled {
            RunLoop.current.run(mode: .default, before: .distantFuture)
        }
    }

    func perform(_ work: @escaping @Sendable () -> Void) {
        guard let loop else { return }
        CFRunLoopPerformBlock(loop, CFRunLoopMode.defaultMode.rawValue, work)
        CFRunLoopWakeUp(loop)
    }
}

final class FieldDrawer: NSObject, CAMetalDisplayLinkDelegate, @unchecked Sendable {
    private static let framesQueuedAhead: Float = 2

    private let layer: CAMetalLayer
    private let renderer: BackdropRenderer
    private let drewFirst: @MainActor @Sendable () -> Void
    private var link: CAMetalDisplayLink?
    private var still = false
    private var toldFirst = false

    init(layer: CAMetalLayer, renderer: BackdropRenderer, drewFirst: @escaping @MainActor @Sendable () -> Void) {
        self.layer = layer
        self.renderer = renderer
        self.drewFirst = drewFirst
    }

    func run(_ motion: BackdropMotion) {
        FieldThread.shared.perform { [self] in apply(motion) }
    }

    func stop() {
        FieldThread.shared.perform { [self] in
            link?.invalidate()
            link = nil
        }
    }

    func restart(_ motion: BackdropMotion) {
        FieldThread.shared.perform { [self] in
            link?.invalidate()
            link = nil
            apply(motion)
        }
    }

    func redrawIfStill() {
        FieldThread.shared.perform { [self] in
            if still { link?.isPaused = false }
        }
    }

    func awaitFirstFrame() {
        FieldThread.shared.perform { [self] in toldFirst = false }
    }

    private func apply(_ motion: BackdropMotion) {
        still = motion == .still
        switch motion {
        case .flowing(let framesPerSecond):
            let rate = Float(framesPerSecond)
            let link = running()
            link.preferredFrameRateRange = CAFrameRateRange(minimum: rate / 2, maximum: rate, preferred: rate)
            link.isPaused = false
        case .still:
            running().isPaused = false
        case .dormant:
            link?.isPaused = true
        }
    }

    private func running() -> CAMetalDisplayLink {
        if let link { return link }
        let started = CAMetalDisplayLink(metalLayer: layer)
        started.preferredFrameLatency = Self.framesQueuedAhead
        started.delegate = self
        started.add(to: .current, forMode: .default)
        link = started
        return started
    }

    func metalDisplayLink(_ link: CAMetalDisplayLink, needsUpdate update: CAMetalDisplayLink.Update) {
        renderer.draw(on: update.drawable, at: update.targetPresentationTimestamp)
        if still { link.isPaused = true }
        guard !toldFirst else { return }
        toldFirst = true
        let drewFirst = drewFirst
        Task { @MainActor in drewFirst() }
    }
}

final class BackdropMetalView: UIView {
    private static let drawingScale: CGFloat = 1
    private static let placeholderLingers: TimeInterval = 0.15

    override static var layerClass: AnyClass { CAMetalLayer.self }

    var motion = BackdropMotion.dormant {
        didSet {
            if motion != oldValue { applyMotion() }
        }
    }

    private let renderer: BackdropRenderer
    private var drawer: FieldDrawer?
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
        drawer = FieldDrawer(layer: metal, renderer: renderer) { [weak self] in self?.liveFrameDrawn() }
        NotificationCenter.default.addObserver(
            self,
            selector: #selector(cameForward),
            name: UIApplication.didBecomeActiveNotification,
            object: nil
        )
    }

    @objc private func cameForward() {
        guard window != nil else { return }
        drawer?.restart(motion)
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

    private func liveFrameDrawn() {
        drewLiveFrame = true
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
            drawer?.awaitFirstFrame()
            showPlaceholder()
        } else {
            keepPlaceholder()
        }
        applyMotion()
    }

    func redrawIfStill() {
        if window != nil, motion == .still { drawer?.redrawIfStill() }
    }

    func stop() {
        drawer?.stop()
    }

    private func applyMotion() {
        guard window != nil else {
            stop()
            return
        }
        drawer?.run(motion)
    }
}

final class BackdropRenderer: Sendable {
    private static let attackSeconds: Float = 0.12
    private static let releaseSeconds: Float = 0.6
    private static let speedSettlingSeconds: Float = 1.2
    private static let restingSpeed: Float = 0.18
    private static let energyReach: Float = 0.25
    private static let fullEnergyLoudness: Float = 0.25
    private static let loopSeconds: Double = 360
    private static let longestStep: CFTimeInterval = 1.0 / 15
    private static let placeholderWidth = 96
    private static let placeholderAspectsAlike: Float = 0.01

    private struct KeptPlaceholder {
        let aspect: Float
        let image: CGImage
    }

    private struct Held {
        var tone = BackdropTone.dark
        var playing = false
        var still = false
        var blend: FieldBlend
        var flow: Double = 0
        var speed = BackdropRenderer.restingSpeed
        var energy: Float = 0
        var lastFrame: CFTimeInterval?
        var kept: KeptPlaceholder?
        var keeping = false
        var keptLook = 0

        mutating func forgetPlaceholder() {
            kept = nil
            keptLook += 1
        }
    }

    private struct Keeping: Sendable {
        let look: Int
        let uniforms: FieldUniforms
    }

    private let gpu: BackdropGPU
    private let heard: HeardLevel
    private let held: Mutex<Held>

    init(gpu: BackdropGPU, palette: Palette, heard: HeardLevel) {
        self.gpu = gpu
        self.heard = heard
        held = Mutex(Held(blend: FieldBlend(palette)))
    }

    func wear(tone: BackdropTone, playing: Bool, still: Bool, palette: Palette) {
        let now = CACurrentMediaTime()

        held.withLock { held in
            let reshaped = tone != held.tone || still != held.still
            held.tone = tone
            held.playing = playing
            held.still = still
            if held.blend.show(palette, at: now) || reshaped { held.forgetPlaceholder() }
        }
    }

    func draw(on drawable: any CAMetalDrawable, at moment: CFTimeInterval) {
        let texture = drawable.texture
        let aspect = texture.height > 0 ? Float(texture.width) / Float(texture.height) : 1
        let loudness = min(heard.rms / Self.fullEnergyLoudness, 1)
        var uniforms = held.withLock { held in
            let step = Float(min(max(moment - (held.lastFrame ?? moment), 0), Self.longestStep))
            held.lastFrame = moment
            Self.advance(&held, by: step, loudness: loudness)
            return Self.uniforms(of: held, at: moment, aspect: aspect)
        }

        let pass = MTLRenderPassDescriptor()
        pass.colorAttachments[0].texture = texture
        pass.colorAttachments[0].loadAction = .dontCare
        pass.colorAttachments[0].storeAction = .store
        guard let commands = gpu.queue.makeCommandBuffer(),
            let encoder = commands.makeRenderCommandEncoder(descriptor: pass)
        else { return }

        encoder.setRenderPipelineState(gpu.pipeline)
        encoder.setFragmentBytes(&uniforms, length: MemoryLayout<FieldUniforms>.size, index: 0)
        encoder.drawPrimitives(type: .triangle, vertexStart: 0, vertexCount: 3)
        encoder.endEncoding()
        BackdropPipeline.reportFailures(of: commands)
        commands.present(drawable)
        commands.commit()
    }

    func placeholder(aspect: Float, at moment: CFTimeInterval) -> CGImage? {
        let kept = held.withLock { held in
            held.kept.flatMap { abs($0.aspect - aspect) < Self.placeholderAspectsAlike ? $0.image : nil }
        }
        if let kept { return kept }
        guard let size = Self.placeholderSize(aspect: aspect) else { return nil }

        let uniforms = held.withLock { Self.uniforms(of: $0, at: moment, aspect: size.aspect) }
        return FieldPicture.renderedNow(by: gpu, uniforms, size: size)
    }

    func keepPlaceholder(aspect: Float) {
        let now = CACurrentMediaTime()
        guard let size = Self.placeholderSize(aspect: aspect) else { return }
        let asked = held.withLock { held -> Keeping? in
            guard !held.keeping, !held.blend.fading(at: now) else { return nil }
            held.keeping = true
            return Keeping(look: held.keptLook, uniforms: Self.uniforms(of: held, at: now, aspect: size.aspect))
        }
        guard let asked else { return }

        let gpu = gpu
        Task { [weak self] in
            let image = await FieldPicture.rendered(by: gpu, asked.uniforms, size: size)
            self?.held.withLock { held in
                held.keeping = false
                if let image, asked.look == held.keptLook { held.kept = KeptPlaceholder(aspect: aspect, image: image) }
            }
        }
    }

    private static func placeholderSize(aspect: Float) -> FieldSize? {
        let height = max(Int((Float(placeholderWidth) / max(aspect, 0.1)).rounded()), 1)
        return FieldSize(CGSize(width: placeholderWidth, height: height))
    }

    private static func advance(_ held: inout Held, by step: Float, loudness: Float) {
        let heard = held.playing && !held.still ? loudness : 0
        let reaction = heard > held.energy ? attackSeconds : releaseSeconds
        let wantedSpeed = held.playing ? 1 + energyReach * held.energy : restingSpeed

        held.energy += (heard - held.energy) * settled(step, within: reaction)
        held.speed += (wantedSpeed - held.speed) * settled(step, within: speedSettlingSeconds)
        held.flow = (held.flow + Double(step * held.speed)).truncatingRemainder(dividingBy: loopSeconds)
    }

    private static func uniforms(of held: Held, at now: CFTimeInterval, aspect: Float) -> FieldUniforms {
        FieldUniforms(
            blend: held.blend,
            time: held.still ? FieldUniforms.stillMoment : Float(held.flow),
            energy: held.still ? 0 : held.energy,
            progress: held.still ? 1 : held.blend.progress(at: now),
            aspect: aspect,
            tone: held.tone
        )
    }

    private static func settled(_ step: Float, within seconds: Float) -> Float {
        Float(1 - exp(-Double(step / seconds)))
    }
}

enum BackdropPipeline {
    static let pixelFormat = MTLPixelFormat.bgra8Unorm
    private static let shaderUniformBytes = 192
    private static let log = Logger(subsystem: "org.resonate.ios", category: "backdrop")
    private static let settledRendering = Mutex<BackdropRendering?>(nil)
    private static let frameFailureLogged = Atomic<Bool>(false)
    private static let compilation = Task.detached(priority: .userInitiated) { settle(compiled()) }

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
