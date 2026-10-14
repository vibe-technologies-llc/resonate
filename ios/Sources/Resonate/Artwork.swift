import ResonateNative
import simd
import SwiftUI
import UIKit

struct ArtworkLoad: Equatable {
    let track: UInt64?
    let revision: UInt64
    let generation: Int
    let portrait: String?
    let portraitGeneration: Int
    let bucket: ArtworkBucket?
}

struct ArtworkView: View {
    @Environment(Library.self) private var library
    @Environment(\.displayScale) private var displayScale
    let track: Track?
    var corner: CGFloat = 12
    var circle = false
    @State private var image: UIImage?
    @State private var bucket: ArtworkBucket?

    var body: some View {
        ZStack {
            Rectangle().fill(.quaternary)
            Image(systemName: circle ? "person.fill" : "music.note")
                .font(.title2)
                .foregroundStyle(.secondary)
            if let image {
                Image(uiImage: image)
                    .resizable()
                    .scaledToFill()
                    .transition(.opacity)
            }
        }
        .clipShape(circle ? AnyShape(Circle()) : AnyShape(RoundedRectangle(cornerRadius: corner, style: .continuous)))
        .animation(.easeInOut(duration: 0.36), value: image)
        .onGeometryChange(for: CGFloat.self) { proxy in
            max(proxy.size.width, proxy.size.height)
        } action: { edge in
            bucket = ArtworkBucket.fitting(points: edge, scale: displayScale)
        }
        .task(id: load) { await show(load) }
        .accessibilityHidden(true)
    }

    private var portraitMbid: String? {
        guard circle, let mbid = track?.artistMbid, !mbid.isEmpty else { return nil }
        return mbid
    }

    private var load: ArtworkLoad {
        let features = library.features
        return ArtworkLoad(
            track: track?.id,
            revision: track?.artRevision ?? 0,
            generation: track.map { features.artGeneration(of: $0.id) } ?? 0,
            portrait: portraitMbid,
            portraitGeneration: portraitMbid == nil ? 0 : features.portraitGeneration,
            bucket: bucket
        )
    }

    private func show(_ load: ArtworkLoad) async {
        guard let bucket = load.bucket else { return }
        guard let track else {
            image = nil
            return
        }
        let features = library.features
        var shown: UIImage?

        if let mbid = load.portrait { shown = await features.portrait(mbid, bucket: bucket) }
        if shown == nil { shown = await features.artwork(track, bucket: bucket) }

        guard !Task.isCancelled else { return }
        image = shown
    }
}

struct Palette: Sendable, Equatable {
    static let size = 5
    private static let accentWeights: [Float] = [0.3, 0.2, 0.2, 0.15, 0.15]
    private static let accentVariations: [(hue: Float, lightness: Float, chroma: Float)] = [
        (0, 0, 1),
        (-0.35, -0.08, 0.85),
        (0.3, 0.05, 0.9),
        (-0.15, 0.1, 0.6),
        (0.5, -0.12, 0.75),
    ]
    private static let darkest: Float = 0.3
    private static let lightest: Float = 0.64
    private static let hueless: Float = 0.015
    private static let dullest: Float = 0.07
    private static let richest: Float = 0.19
    private static let chromaLift: Float = 1.2
    private static let gamutSteps = 12
    private static let gamutShrink: Float = 0.88

    let colours: [SIMD3<Float>]
    let weights: [Float]

    private init(colours: [SIMD3<Float>], weights: [Float]) {
        self.colours = colours
        self.weights = weights
    }

    static func around(accent: SIMD3<Float>) -> Palette {
        let lab = Oklab.lab(fromLinear: accent)
        let chroma = (lab.y * lab.y + lab.z * lab.z).squareRoot()
        let hue = Float(atan2(Double(lab.z), Double(lab.y)))
        let colours = accentVariations.map { variation in
            richened(Oklab.polar(
                lightness: lab.x + variation.lightness,
                chroma: chroma * variation.chroma,
                hue: hue + variation.hue
            ))
        }
        return Palette(colours: colours, weights: accentWeights)
    }

    static func shaped(_ clusters: [(centre: SIMD3<Float>, members: Int)], smallestWeight: Float) -> Palette {
        let ranked = clusters.sorted { $0.members > $1.members }
        let total = Float(max(ranked.reduce(0) { $0 + $1.members }, 1))
        let raised = ranked.map { max(Float($0.members) / total, smallestWeight) }
        let sum = raised.reduce(0, +)

        return Palette(colours: ranked.map { richened($0.centre) }, weights: raised.map { $0 / sum })
    }

    func blended(toward other: Palette, by amount: Float) -> Palette {
        Palette(
            colours: zip(colours, other.colours).map { $0 + ($1 - $0) * amount },
            weights: zip(weights, other.weights).map { $0 + ($1 - $0) * amount }
        )
    }

    private static func richened(_ lab: SIMD3<Float>) -> SIMD3<Float> {
        let chroma = (lab.y * lab.y + lab.z * lab.z).squareRoot()
        let hue = Float(atan2(Double(lab.z), Double(lab.y)))
        let lightness = min(max(lab.x, darkest), lightest)
        var fitted = chroma < hueless ? chroma : min(max(chroma * chromaLift, dullest), richest)

        for _ in 0..<gamutSteps {
            let linear = Oklab.linear(fromLab: Oklab.polar(lightness: lightness, chroma: fitted, hue: hue))
            if linear.min() >= 0, linear.max() <= 1 { return linear }
            fitted *= gamutShrink
        }
        let linear = Oklab.linear(fromLab: Oklab.polar(lightness: lightness, chroma: fitted, hue: hue))
        return simd_clamp(linear, SIMD3(repeating: 0), SIMD3(repeating: 1))
    }
}

enum Oklab {
    static func lab(fromLinear rgb: SIMD3<Float>) -> SIMD3<Float> {
        let lms = SIMD3<Float>(
            0.4122214708 * rgb.x + 0.5363325363 * rgb.y + 0.0514459929 * rgb.z,
            0.2119034982 * rgb.x + 0.6806995451 * rgb.y + 0.1073969566 * rgb.z,
            0.0883024619 * rgb.x + 0.2817188376 * rgb.y + 0.6299787005 * rgb.z
        )
        let root = SIMD3<Float>(cubeRoot(lms.x), cubeRoot(lms.y), cubeRoot(lms.z))
        return SIMD3(
            0.2104542553 * root.x + 0.7936177850 * root.y - 0.0040720468 * root.z,
            1.9779984951 * root.x - 2.4285922050 * root.y + 0.4505937099 * root.z,
            0.0259040371 * root.x + 0.7827717662 * root.y - 0.8086757660 * root.z
        )
    }

    static func linear(fromLab lab: SIMD3<Float>) -> SIMD3<Float> {
        let root = SIMD3<Float>(
            lab.x + 0.3963377774 * lab.y + 0.2158037573 * lab.z,
            lab.x - 0.1055613458 * lab.y - 0.0638541728 * lab.z,
            lab.x - 0.0894841775 * lab.y - 1.2914855480 * lab.z
        )
        let lms = root * root * root
        return SIMD3(
            4.0767416621 * lms.x - 3.3077115913 * lms.y + 0.2309699292 * lms.z,
            -1.2684380046 * lms.x + 2.6097574011 * lms.y - 0.3413193965 * lms.z,
            -0.0041960863 * lms.x - 0.7034186147 * lms.y + 1.7076147010 * lms.z
        )
    }

    static func polar(lightness: Float, chroma: Float, hue: Float) -> SIMD3<Float> {
        SIMD3(lightness, chroma * Float(cos(Double(hue))), chroma * Float(sin(Double(hue))))
    }

    static func chroma(of lab: SIMD3<Float>) -> Float {
        (lab.y * lab.y + lab.z * lab.z).squareRoot()
    }

    private static func cubeRoot(_ value: Float) -> Float {
        Float(cbrt(Double(value)))
    }
}

enum Backdrop {
    private static let sampleEdge = 32
    private static let rounds = 6
    private static let opaqueEnough: UInt8 = 128
    private static let blackest: Float = 0.18
    private static let whitest: Float = 0.93
    private static let greyest: Float = 0.035
    private static let colourlessBelowOneIn = 16
    private static let vividOnlyFromOneIn = 3
    private static let smallestWeight: Float = 0.06
    private static let linearOfByte: [Float] = (0...255).map { byte in
        let encoded = Float(byte) / 255
        return encoded <= 0.04045 ? encoded / 12.92 : Float(pow((Double(encoded) + 0.055) / 1.055, 2.4))
    }

    static func palette(of image: UIImage, accent: SIMD3<Float>) async -> Palette {
        await Task.detached(priority: .utility) { sampled(image) ?? .around(accent: accent) }.value
    }

    private static func sampled(_ image: UIImage) -> Palette? {
        guard let pixels = labPixels(of: image), !pixels.isEmpty else { return nil }
        let vivid = pixels.filter(isVivid)

        guard vivid.count * colourlessBelowOneIn >= pixels.count else { return nil }
        let chosen = vivid.count * vividOnlyFromOneIn >= pixels.count ? vivid : pixels.filter(isMidtone)
        return Palette.shaped(clustered(chosen), smallestWeight: smallestWeight)
    }

    private static func isMidtone(_ lab: SIMD3<Float>) -> Bool {
        lab.x > blackest && lab.x < whitest
    }

    private static func isVivid(_ lab: SIMD3<Float>) -> Bool {
        isMidtone(lab) && Oklab.chroma(of: lab) >= greyest
    }

    private static func labPixels(of image: UIImage) -> [SIMD3<Float>]? {
        guard let source = image.cgImage, let space = CGColorSpace(name: CGColorSpace.sRGB) else { return nil }
        let edge = sampleEdge
        var bytes = [UInt8](repeating: 0, count: edge * edge * 4)

        let drawn = bytes.withUnsafeMutableBytes { buffer -> Bool in
            guard let context = CGContext(
                data: buffer.baseAddress,
                width: edge,
                height: edge,
                bitsPerComponent: 8,
                bytesPerRow: edge * 4,
                space: space,
                bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
            ) else { return false }
            context.interpolationQuality = .medium
            context.draw(source, in: CGRect(x: 0, y: 0, width: edge, height: edge))
            return true
        }
        guard drawn else { return nil }

        return stride(from: 0, to: bytes.count, by: 4).compactMap { at in
            let alpha = bytes[at + 3]
            guard alpha >= opaqueEnough else { return nil }
            let rgb = SIMD3<Float>(
                linearOfByte[unpremultiplied(bytes[at], alpha)],
                linearOfByte[unpremultiplied(bytes[at + 1], alpha)],
                linearOfByte[unpremultiplied(bytes[at + 2], alpha)]
            )
            return Oklab.lab(fromLinear: rgb)
        }
    }

    private static func unpremultiplied(_ channel: UInt8, _ alpha: UInt8) -> Int {
        min(Int(channel) * 255 / Int(alpha), 255)
    }

    private static func clustered(_ points: [SIMD3<Float>]) -> [(centre: SIMD3<Float>, members: Int)] {
        let byLightness = points.sorted { $0.x < $1.x }
        var centres = (0..<Palette.size).map { index in
            byLightness[min((2 * index + 1) * byLightness.count / (2 * Palette.size), byLightness.count - 1)]
        }
        var members = [Int](repeating: 0, count: Palette.size)

        for _ in 0..<rounds {
            var sums = [SIMD3<Float>](repeating: .zero, count: Palette.size)
            members = [Int](repeating: 0, count: Palette.size)
            for point in points {
                let nearest = nearestCentre(to: point, among: centres)
                sums[nearest] += point
                members[nearest] += 1
            }
            for index in 0..<Palette.size where members[index] > 0 {
                centres[index] = sums[index] / Float(members[index])
            }
        }
        return zip(centres, members).map { (centre: $0, members: $1) }
    }

    private static func nearestCentre(to point: SIMD3<Float>, among centres: [SIMD3<Float>]) -> Int {
        centres.indices.min { simd_distance_squared(point, centres[$0]) < simd_distance_squared(point, centres[$1]) } ?? 0
    }
}

struct FavouriteButton: View {
    @Environment(Library.self) private var library
    let track: Track

    var body: some View {
        Button {
            library.setFavourite(track, !track.favourite)
        } label: {
            Image(systemName: track.favourite ? "heart.fill" : "heart")
                .foregroundStyle(track.favourite ? Color.pink : Color.secondary)
                .contentTransition(.symbolEffect(.replace))
        }
        .buttonStyle(.plain)
        .sensoryFeedback(.selection, trigger: track.favourite)
        .accessibilityLabel(track.favourite ? "Remove from favourites" : "Add to favourites")
    }
}
