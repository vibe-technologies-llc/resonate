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
    let ambience: Float?
    @State private var palette: Palette?
    @State private var rendering: BackdropRendering
    @State private var lowPower = ProcessInfo.processInfo.sparesFrames
    @State private var visible = false
    @State private var listening = false

    init(track: Track?, moving: Bool = true, ambience: Float? = nil) {
        self.track = track
        self.moving = moving
        self.ambience = ambience
        _palette = State(initialValue: track.flatMap { PaletteMemory.recalled(Self.key(of: $0)) } ?? Self.accentPalette())
        _rendering = State(initialValue: BackdropPipeline.finished ?? .loading)
    }

    var body: some View {
        ZStack {
            Color(.systemBackground)
            if let palette {
                if case .metal(let gpu) = rendering {
                    MeshBackdrop(
                        gpu: gpu,
                        library: library,
                        palette: palette,
                        motion: motion,
                        tone: tone,
                        reactive: ambience == nil,
                        playing: sounding
                    )
                    .transition(.opacity)
                } else {
                    GradientBackdrop(palette: palette, weight: ambience ?? 1)
                }
            }
        }
        .ignoresSafeArea()
        .animation(.easeInOut(duration: 0.4), value: palette == nil)
        .animation(.easeInOut(duration: 0.4), value: rendering.drawsMesh)
        .task { await prepareRendering() }
        .task(id: "\(track.map(Self.key(of:)) ?? ""):\(library.features.artGeneration(of: track?.id ?? 0)):\(AppearanceState.shared.accent?.name ?? "")") { await loadPalette() }
        .onAppear { visible = true }
        .onDisappear {
            visible = false
            listen(false)
        }
        .onChange(of: wantsEnergy) { _, wanted in listen(wanted) }
        .onReceive(Self.powerStateChanges) { _ in lowPower = ProcessInfo.processInfo.sparesFrames }
    }

    private static var powerStateChanges: some Publisher<Notification, Never> {
        NotificationCenter.default
            .publisher(for: .NSProcessInfoPowerStateDidChange)
            .merge(with: NotificationCenter.default.publisher(for: ProcessInfo.thermalStateDidChangeNotification))
            .receive(on: DispatchQueue.main)
    }

    private static func key(of track: Track) -> String {
        "\(track.id):\(track.artRevision)"
    }

    private var tone: BackdropTone {
        if let ambience { return .ambient(scheme, strength: ambience) }
        return scheme == .dark ? .dark : .light
    }

    private var motion: BackdropMotion {
        guard phase == .active else { return .dormant }
        if reduceMotion || !moving { return .still }
        return .flowing(framesPerSecond: Self.rate(sounding: sounding, ambient: ambience != nil, lowPower: lowPower))
    }

    private var sounding: Bool {
        library.transport == .playing
    }

    private static func rate(sounding: Bool, ambient: Bool, lowPower: Bool) -> Int {
        switch (ambient, sounding, lowPower) {
        case (true, true, false): 20
        case (true, _, _): 10
        case (false, true, false): 30
        case (false, true, true): 20
        case (false, false, false): 15
        case (false, false, true): 10
        }
    }

    private var wantsEnergy: Bool {
        visible && moving && ambience == nil && phase == .active && !reduceMotion && rendering.drawsMesh && sounding
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

    private static func accentLinear() -> SIMD3<Float> {
        let accent = Color.worn.resolve(in: EnvironmentValues())
        return SIMD3<Float>(accent.linearRed, accent.linearGreen, accent.linearBlue)
    }

    private static func accentPalette() -> Palette {
        AccentPaletteMemory.palette(around: accentLinear())
    }

    private func loadPalette() async {
        let accentLinear = Self.accentLinear()

        let found = if let track { await library.features.artwork(track, bucket: .tile) } else { nil as UIImage? }
        guard !Task.isCancelled else { return }

        guard let track, let image = found else {
            let around = AccentPaletteMemory.palette(around: accentLinear)
            if palette != around { palette = around }
            return
        }
        let sampled = await Backdrop.palette(of: image, accent: accentLinear)
        guard !Task.isCancelled else { return }

        PaletteMemory.remember(sampled, as: Self.key(of: track))
        palette = sampled
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

private struct MeshBackdrop: UIViewRepresentable {
    let gpu: BackdropGPU
    let library: Library
    let palette: Palette
    let motion: BackdropMotion
    let tone: BackdropTone
    let reactive: Bool
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
        renderer.reactive = reactive
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

    override static var layerClass: AnyClass { CAMetalLayer.self }

    var motion = BackdropMotion.dormant {
        didSet {
            if motion != oldValue { applyMotion() }
        }
    }

    private let renderer: BackdropRenderer
    private var link: CAMetalDisplayLink?
    private var drewFirstFrame = false

    init(device: any MTLDevice, renderer: BackdropRenderer) {
        self.renderer = renderer
        super.init(frame: .zero)
        isOpaque = false
        backgroundColor = .clear
        isUserInteractionEnabled = false
        contentScaleFactor = Self.drawingScale
        guard let metal = metalLayer else { return }
        metal.device = device
        metal.pixelFormat = BackdropPipeline.pixelFormat
        metal.framebufferOnly = true
        metal.isOpaque = false
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
        drawFirstFrame()
        redrawIfStill()
    }

    private func drawFirstFrame() {
        guard !drewFirstFrame, window != nil, UIApplication.shared.applicationState == .active,
            let metal = metalLayer, metal.drawableSize.width >= 1, metal.drawableSize.height >= 1,
            let drawable = metal.nextDrawable()
        else { return }
        drewFirstFrame = true
        metal.presentsWithTransaction = true
        renderer.draw(on: drawable, at: CACurrentMediaTime(), withTransaction: true)
        metal.presentsWithTransaction = false
    }

    override func didMoveToWindow() {
        super.didMoveToWindow()
        contentScaleFactor = Self.drawingScale
        metalLayer?.contentsScale = Self.drawingScale
        drawFirstFrame()
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
    private static let crossfadeSeconds: CFTimeInterval = 1.2
    private static let attackSeconds: Float = 0.12
    private static let releaseSeconds: Float = 0.6
    private static let speedSettlingSeconds: Float = 1.2
    private static let restingSpeed: Float = 0.18
    private static let energyReach: Float = 0.25
    private static let fullEnergyLoudness: Float = 0.25
    private static let loopSeconds: Double = 360
    private static let stillMoment: Float = 13
    private static let longestStep: CFTimeInterval = 1.0 / 15

    weak var library: Library?
    var tone = BackdropTone.dark
    var reactive = true
    var playing = false
    var still = false

    private let gpu: BackdropGPU
    private var shown: Palette
    private var leaving: Palette
    private var shownPacked: FieldUniforms.Five
    private var leavingPacked: FieldUniforms.Five
    private var crossfadeBegan = -CFTimeInterval.infinity
    private var flow: Double = 0
    private var speed = BackdropRenderer.restingSpeed
    private var energy: Float = 0
    private var lastFrame: CFTimeInterval?

    init(gpu: BackdropGPU, palette: Palette) {
        self.gpu = gpu
        shown = palette
        leaving = palette
        shownPacked = FieldUniforms.packed(palette)
        leavingPacked = shownPacked
    }

    func show(_ palette: Palette) {
        guard palette != shown else { return }
        let now = CACurrentMediaTime()
        let reached = Self.eased(crossfade(at: now))

        leaving = leaving.blended(toward: shown, by: reached)
        shown = palette
        leavingPacked = FieldUniforms.packed(leaving)
        shownPacked = FieldUniforms.packed(shown)
        crossfadeBegan = now
    }

    func draw(on drawable: any CAMetalDrawable, at moment: CFTimeInterval, withTransaction: Bool = false) {
        let step = Float(min(max(moment - (lastFrame ?? moment), 0), Self.longestStep))

        lastFrame = moment
        advance(by: step, at: moment)

        let pass = MTLRenderPassDescriptor()
        pass.colorAttachments[0].texture = drawable.texture
        pass.colorAttachments[0].loadAction = .dontCare
        pass.colorAttachments[0].storeAction = .store
        guard let commands = gpu.queue.makeCommandBuffer(),
            let encoder = commands.makeRenderCommandEncoder(descriptor: pass)
        else { return }
        let size = CGSize(width: drawable.texture.width, height: drawable.texture.height)
        let uniforms = self.uniforms(at: moment, drawable: size)

        encoder.setRenderPipelineState(gpu.pipeline)
        withUnsafeBytes(of: uniforms) { bytes in
            guard let base = bytes.baseAddress else { return }
            encoder.setFragmentBytes(base, length: bytes.count, index: 0)
        }
        encoder.drawPrimitives(type: .triangle, vertexStart: 0, vertexCount: 3)
        encoder.endEncoding()
        BackdropPipeline.reportFailures(of: commands)
        if withTransaction {
            commands.commit()
            commands.waitUntilScheduled()
            drawable.present()
        } else {
            commands.present(drawable)
            commands.commit()
        }
    }

    private func advance(by step: Float, at moment: CFTimeInterval) {
        let heard = playing && !still && reactive ? loudness() : 0
        let reaction = heard > energy ? Self.attackSeconds : Self.releaseSeconds
        let wantedSpeed = playing ? 1 + Self.energyReach * energy : Self.restingSpeed

        energy += (heard - energy) * Self.settled(step, within: reaction)
        speed += (wantedSpeed - speed) * Self.settled(step, within: Self.speedSettlingSeconds)
        if reactive {
            flow = (flow + Double(step * speed)).truncatingRemainder(dividingBy: Self.loopSeconds)
        } else {
            flow = (moment * Double(Self.restingSpeed)).truncatingRemainder(dividingBy: Self.loopSeconds)
        }
    }

    private func loudness() -> Float {
        guard let rms = library?.features.heardRms else { return 0 }
        return min(rms / Self.fullEnergyLoudness, 1)
    }

    private func crossfade(at now: CFTimeInterval) -> Float {
        Float(min(max((now - crossfadeBegan) / Self.crossfadeSeconds, 0), 1))
    }

    private func uniforms(at now: CFTimeInterval, drawable size: CGSize) -> FieldUniforms {
        let aspect = size.width > 0 && size.height > 0 ? Float(size.width / size.height) : 1

        return FieldUniforms(
            colours: shownPacked,
            previous: leavingPacked,
            time: still ? Self.stillMoment : Float(flow),
            energy: still ? 0 : energy,
            progress: still ? 1 : crossfade(at: now),
            aspect: aspect,
            lightnessFloor: tone.lightnessFloor,
            lightnessRange: tone.lightnessRange,
            chroma: tone.chroma,
            vignette: tone.vignette
        )
    }

    private static func settled(_ step: Float, within seconds: Float) -> Float {
        Float(1 - exp(-Double(step / seconds)))
    }

    private static func eased(_ progress: Float) -> Float {
        progress * progress * (3 - 2 * progress)
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
}
