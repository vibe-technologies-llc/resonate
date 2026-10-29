import ResonateLogic
import ResonateNative
import simd
import SwiftUI
import UIKit

struct ArtworkLoad: Equatable {
    let track: UInt64?
    let served: String?
    let revision: UInt64
    let generation: Int
    let portrait: String?
    let portraitGeneration: Int
    let bucket: ArtworkBucket?
}

struct ArtworkView: View {
    private static let arrival: TimeInterval = 0.2
    private static let settlesWithin: Duration = .milliseconds(300)

    @Environment(Library.self) private var library
    @Environment(\.displayScale) private var displayScale
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    let track: Track?
    var corner: CGFloat = 12
    var circle = false
    var handsOver = false
    @State private var image: UIImage?
    @State private var pictured: String?
    @State private var leaving: UIImage?
    @State private var face = 1
    @State private var bucket: ArtworkBucket?
    @State private var wanted: ArtworkBucket?

    init(track: Track?, corner: CGFloat = 12, circle: Bool = false, handsOver: Bool = false) {
        self.track = track
        self.corner = corner
        self.circle = circle
        self.handsOver = handsOver
        let latest = circle ? nil : ShownArtwork.latest(for: track)
        _image = State(initialValue: latest)
        _pictured = State(initialValue: latest == nil ? nil : track?.pictureKey)
    }

    var body: some View {
        Rectangle()
            .fill(.quaternary)
            .overlay {
                Image(systemName: circle ? "person.fill" : "music.note")
                    .font(.title2)
                    .foregroundStyle(.secondary)
            }
            .overlay {
                if let leaving {
                    Image(uiImage: leaving)
                        .resizable()
                        .scaledToFill()
                }
            }
            .overlay {
                ZStack {
                    if let image {
                        Image(uiImage: image)
                            .resizable()
                            .scaledToFill()
                            .id(face)
                            .zIndex(Double(face))
                            .transition(.opacity)
                    }
                }
            }
            .modifier(ArtworkClip(circle: circle, corner: corner))
        .onGeometryChange(for: CGFloat.self) { proxy in
            max(proxy.size.width, proxy.size.height)
        } action: { edge in
            measured(ArtworkBucket.fitting(points: edge, scale: displayScale))
        }
        .task(id: load) { await show(load) }
        .task(id: wanted) { await grow(to: wanted) }
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
            served: track?.isServed == true ? track?.uri : nil,
            revision: track?.artRevision ?? 0,
            generation: track.map { $0.isServed ? features.servedGeneration : features.artGeneration(of: $0.id) } ?? 0,
            portrait: portraitMbid,
            portraitGeneration: portraitMbid == nil ? 0 : features.portraitGeneration,
            bucket: bucket
        )
    }

    private func measured(_ fitting: ArtworkBucket) {
        guard let shown = bucket, image != nil else {
            bucket = fitting
            wanted = nil
            if image == nil, let held = held(at: fitting) {
                image = held
                pictured = track?.pictureKey
            }
            return
        }
        guard fitting.rawValue > shown.rawValue else {
            wanted = nil
            return
        }
        if let held = held(at: fitting) {
            bucket = fitting
            wanted = nil
            image = held
        } else {
            wanted = fitting
        }
    }

    private func grow(to target: ArtworkBucket?) async {
        guard let target else { return }
        try? await Task.sleep(for: Self.settlesWithin)
        guard !Task.isCancelled else { return }
        bucket = target
    }

    private func held(at bucket: ArtworkBucket) -> UIImage? {
        guard let track else { return nil }
        let features = library.features
        if let mbid = portraitMbid, let portrait = features.cachedPortrait(mbid, bucket: bucket) {
            return portrait
        }
        return features.cachedArtwork(track, bucket: bucket)
    }

    private func show(_ load: ArtworkLoad) async {
        guard let bucket = load.bucket else { return }
        guard let track else {
            image = nil
            return
        }
        if let held = held(at: bucket) {
            if image !== held { present(held, of: track) }
            return
        }
        let features = library.features
        var shown: UIImage?

        if let mbid = load.portrait { shown = await features.portrait(mbid, bucket: bucket) }
        let portrayed = shown != nil
        if shown == nil { shown = await features.artwork(track, bucket: bucket) }

        guard !Task.isCancelled else { return }
        if image == nil, shown != nil {
            withAnimation(.easeOut(duration: Self.arrival)) { image = shown }
            pictured = track.pictureKey
        } else if image !== shown {
            present(shown, of: track)
        }
        if let shown, !portrayed { ShownArtwork.keep(shown, for: track) }
    }

    private func present(_ shown: UIImage?, of track: Track) {
        let anotherSong = pictured != track.pictureKey
        pictured = shown == nil ? nil : track.pictureKey
        guard handsOver, anotherSong, let current = image, !reduceMotion else {
            image = shown
            return
        }
        leaving = shown == nil ? nil : current
        let handedOver = face + 1
        withAnimation(Handover.animation, completionCriteria: .logicallyComplete) {
            image = shown
            face = handedOver
        } completion: {
            if face == handedOver { leaving = nil }
        }
    }
}

struct ArtworkClip: ViewModifier {
    let circle: Bool
    let corner: CGFloat

    func body(content: Content) -> some View {
        if circle {
            content.clipShape(.circle)
        } else {
            content.clipShape(.rect(cornerRadius: corner, style: .continuous))
        }
    }
}

@MainActor
enum ShownArtwork {
    private static let budget = 64 * 1024 * 1024

    private final class Shown {
        let revision: UInt64
        let image: UIImage

        init(revision: UInt64, image: UIImage) {
            self.revision = revision
            self.image = image
        }
    }

    private static let kept: NSCache<NSNumber, Shown> = {
        let cache = NSCache<NSNumber, Shown>()
        cache.totalCostLimit = budget
        return cache
    }()

    static func latest(for track: Track?) -> UIImage? {
        guard let track, !track.isServed, let shown = kept.object(forKey: NSNumber(value: track.id)), shown.revision == track.artRevision else {
            return nil
        }
        return shown.image
    }

    static func keep(_ image: UIImage, for track: Track) {
        guard !track.isServed else { return }
        let key = NSNumber(value: track.id)
        if let held = kept.object(forKey: key), held.revision == track.artRevision, edge(of: held.image) >= edge(of: image) {
            return
        }
        kept.setObject(Shown(revision: track.artRevision, image: image), forKey: key, cost: ArtworkDecoder.cost(of: image))
    }

    private static func edge(of image: UIImage) -> CGFloat {
        max(image.size.width, image.size.height) * image.scale
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

        guard vivid.count * colourlessBelowOneIn >= pixels.count else {
            return Palette.shaped(clustered(pixels), smallestWeight: smallestWeight, darkest: Palette.darkestGrey)
        }
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
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    let track: Track
    @State private var tapped = 0

    var body: some View {
        if !track.isServed {
            heart
        }
    }

    private var heart: some View {
        let favourite = library.isFavourite(track)
        return Button {
            tapped += 1
            library.setFavourite(track, !favourite)
        } label: {
            Image(systemName: favourite ? "heart.fill" : "heart")
                .foregroundStyle(favourite ? Color.pink : Color.secondary)
                .contentTransition(reduceMotion ? .identity : .symbolEffect(.replace))
                .symbolEffect(.bounce, value: reduceMotion ? 0 : tapped)
                .touchTarget()
        }
        .buttonStyle(.plain)
        .sensoryFeedback(.selection, trigger: tapped)
        .accessibilityLabel(favourite ? "Remove from favourites" : "Add to favourites")
    }
}
