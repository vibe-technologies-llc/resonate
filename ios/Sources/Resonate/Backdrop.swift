import Combine
import Metal
import MetalKit
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
    @State private var palette: Palette?
    @State private var rendering: BackdropRendering
    @State private var lowPower = ProcessInfo.processInfo.isLowPowerModeEnabled
    @State private var visible = false
    @State private var listening = false

    init(track: Track?) {
        self.track = track
        _palette = State(initialValue: track.flatMap { PaletteMemory.recalled(Self.key(of: $0)) })
        _rendering = State(initialValue: BackdropPipeline.finished ?? .loading)
    }

    var body: some View {
        ZStack {
            Color(.systemBackground)
            if let palette {
                GradientBackdrop(palette: palette)
                if case .metal(let gpu) = rendering {
                    MeshBackdrop(
                        gpu: gpu,
                        library: library,
                        palette: palette,
                        motion: motion,
                        tone: scheme == .dark ? .dark : .light
                    )
                    .transition(.opacity)
                }
            }
        }
        .ignoresSafeArea()
        .animation(.easeInOut(duration: 0.4), value: palette == nil)
        .animation(.easeInOut(duration: 0.4), value: rendering.drawsMesh)
        .task { await prepareRendering() }
        .task(id: "\(track.map(Self.key(of:)) ?? ""):\(library.features.artworkGeneration)") { await loadPalette() }
        .onAppear { visible = true }
        .onDisappear {
            visible = false
            listen(false)
        }
        .onChange(of: wantsEnergy) { _, wanted in listen(wanted) }
        .onReceive(Self.powerStateChanges) { _ in lowPower = ProcessInfo.processInfo.isLowPowerModeEnabled }
    }

    private static var powerStateChanges: some Publisher<Notification, Never> {
        NotificationCenter.default
            .publisher(for: .NSProcessInfoPowerStateDidChange)
            .receive(on: DispatchQueue.main)
    }

    private static func key(of track: Track) -> String {
        "\(track.id):\(track.artRevision)"
    }

    private var motion: BackdropMotion {
        guard phase == .active else { return .dormant }
        if reduceMotion { return .still }
        return .flowing(framesPerSecond: lowPower ? 30 : 60)
    }

    private var wantsEnergy: Bool {
        visible && phase == .active && !reduceMotion && rendering.drawsMesh
    }

    private func listen(_ wanted: Bool) {
        guard wanted != listening else { return }
        listening = wanted
        library.features.visualize(wanted)
    }

    private func prepareRendering() async {
        guard case .loading = rendering else { return }
        rendering = await BackdropPipeline.rendering()
    }

    private func loadPalette() async {
        let accent = Color.accentColor.resolve(in: EnvironmentValues())
        let accentLinear = SIMD3<Float>(accent.linearRed, accent.linearGreen, accent.linearBlue)

        guard let track, let image = await library.features.artwork(track) else {
            palette = Palette.around(accent: accentLinear)
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

    var body: some View {
        ZStack {
            RadialGradient(
                colors: [Self.color(palette.colours[0]).opacity(0.55), .clear],
                center: .topLeading,
                startRadius: 0,
                endRadius: 600
            )
            RadialGradient(
                colors: [Self.color(palette.colours[1]).opacity(0.35), .clear],
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

    func makeCoordinator() -> BackdropRenderer {
        BackdropRenderer(gpu: gpu, palette: palette)
    }

    func makeUIView(context: Context) -> BackdropMetalView {
        let view = BackdropMetalView(frame: .zero, device: gpu.device)
        view.colorPixelFormat = BackdropPipeline.pixelFormat
        view.framebufferOnly = true
        view.autoResizeDrawable = true
        view.isOpaque = false
        view.backgroundColor = .clear
        view.isUserInteractionEnabled = false
        view.isPaused = true
        view.enableSetNeedsDisplay = false
        view.delegate = context.coordinator
        return view
    }

    func updateUIView(_ view: BackdropMetalView, context: Context) {
        let renderer = context.coordinator

        renderer.library = library
        renderer.tone = tone
        renderer.still = motion == .still
        renderer.show(palette)
        view.motion = motion
        view.redrawIfStill()
    }

    static func dismantleUIView(_ view: BackdropMetalView, coordinator: BackdropRenderer) {
        view.motion = .dormant
        view.delegate = nil
    }
}

final class BackdropMetalView: MTKView {
    private static let drawingScale: CGFloat = 1

    var motion = BackdropMotion.dormant {
        didSet {
            if motion != oldValue { applyMotion() }
        }
    }

    override func didMoveToWindow() {
        super.didMoveToWindow()
        contentScaleFactor = Self.drawingScale
        applyMotion()
    }

    func redrawIfStill() {
        if window != nil, motion == .still { setNeedsDisplay() }
    }

    private func applyMotion() {
        switch window == nil ? .dormant : motion {
        case .flowing(let framesPerSecond):
            preferredFramesPerSecond = framesPerSecond
            enableSetNeedsDisplay = false
            isPaused = false
        case .still:
            isPaused = true
            enableSetNeedsDisplay = true
            setNeedsDisplay()
        case .dormant:
            isPaused = true
            enableSetNeedsDisplay = false
        }
    }
}

@MainActor
final class BackdropRenderer: NSObject, MTKViewDelegate {
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
    var still = false

    private let gpu: BackdropGPU
    private var shown: Palette
    private var leaving: Palette
    private var crossfadeBegan = -CFTimeInterval.infinity
    private var flow: Double = 0
    private var speed = BackdropRenderer.restingSpeed
    private var energy: Float = 0
    private var lastFrame: CFTimeInterval?

    init(gpu: BackdropGPU, palette: Palette) {
        self.gpu = gpu
        shown = palette
        leaving = palette
    }

    func show(_ palette: Palette) {
        guard palette != shown else { return }
        let now = CACurrentMediaTime()
        let reached = Self.eased(crossfade(at: now))

        leaving = leaving.blended(toward: shown, by: reached)
        shown = palette
        crossfadeBegan = now
    }

    func mtkView(_ view: MTKView, drawableSizeWillChange size: CGSize) {}

    func draw(in view: MTKView) {
        let now = CACurrentMediaTime()
        let step = Float(min(now - (lastFrame ?? now), Self.longestStep))

        lastFrame = now
        advance(by: step)

        guard let pass = view.currentRenderPassDescriptor,
            let drawable = view.currentDrawable,
            let commands = gpu.queue.makeCommandBuffer()
        else { return }
        pass.colorAttachments[0].loadAction = .dontCare
        guard let encoder = commands.makeRenderCommandEncoder(descriptor: pass) else { return }
        let uniforms = self.uniforms(at: now, drawable: view.drawableSize)

        encoder.setRenderPipelineState(gpu.pipeline)
        withUnsafeBytes(of: uniforms) { bytes in
            guard let base = bytes.baseAddress else { return }
            encoder.setFragmentBytes(base, length: bytes.count, index: 0)
        }
        encoder.drawPrimitives(type: .triangle, vertexStart: 0, vertexCount: 3)
        encoder.endEncoding()
        BackdropPipeline.reportFailures(of: commands)
        commands.present(drawable)
        commands.commit()
    }

    private func advance(by step: Float) {
        let playing = library?.snapshot?.transport == .playing
        let heard = playing && !still ? loudness() : 0
        let reaction = heard > energy ? Self.attackSeconds : Self.releaseSeconds
        let wantedSpeed = playing ? 1 + Self.energyReach * energy : Self.restingSpeed

        energy += (heard - energy) * Self.settled(step, within: reaction)
        speed += (wantedSpeed - speed) * Self.settled(step, within: Self.speedSettlingSeconds)
        flow = (flow + Double(step * speed)).truncatingRemainder(dividingBy: Self.loopSeconds)
    }

    private func loudness() -> Float {
        guard let samples = library?.features.samples, !samples.isEmpty else { return 0 }
        let power = samples.reduce(0) { $0 + $1 * $1 } / Float(samples.count)

        return min(power.squareRoot() / Self.fullEnergyLoudness, 1)
    }

    private func crossfade(at now: CFTimeInterval) -> Float {
        Float(min(max((now - crossfadeBegan) / Self.crossfadeSeconds, 0), 1))
    }

    private func uniforms(at now: CFTimeInterval, drawable size: CGSize) -> FieldUniforms {
        let aspect = size.width > 0 && size.height > 0 ? Float(size.width / size.height) : 1

        return FieldUniforms(
            colours: FieldUniforms.packed(shown),
            previous: FieldUniforms.packed(leaving),
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
            SIMD4<Float>(colour.x, colour.y, colour.z, weight)
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

        float3 oklabFromLinear(float3 rgb) {
            float3 lms = float3(
                dot(rgb, float3(0.4122214708, 0.5363325363, 0.0514459929)),
                dot(rgb, float3(0.2119034982, 0.6806995451, 0.1073969566)),
                dot(rgb, float3(0.0883024619, 0.2817188376, 0.6299787005)));
            float3 root = powr(max(lms, float3(0.0000001)), float3(1.0 / 3.0));
            return float3(
                dot(root, float3(0.2104542553, 0.7936177850, -0.0040720468)),
                dot(root, float3(1.9779984951, -2.4285922050, 0.4505937099)),
                dot(root, float3(0.0259040371, 0.7827717662, -0.8086757660)));
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

        float3 fieldOf(constant float4 *colours, float2 place, float time, float energy, float2 frame) {
            float growth = 1.0 + energyGrowth * energy;
            float3 blended = float3(0.0);
            float3 ground = float3(0.0);
            float blendedChroma = 0.0;
            float groundChroma = 0.0;
            float total = 0.0;
            float weights = 0.0;
            for (int index = 0; index < blobCount; index++) {
                float4 colour = colours[index];
                float3 lab = oklabFromLinear(colour.rgb);
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
