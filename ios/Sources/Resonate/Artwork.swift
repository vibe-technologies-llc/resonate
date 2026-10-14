import ResonateNative
import SwiftUI
import UIKit

struct ArtworkView: View {
    @Environment(Library.self) private var library
    let track: Track?
    var corner: CGFloat = 12
    var circle = false
    @State private var image: UIImage?

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
        .task(id: "\(track?.id ?? 0):\(track?.artRevision ?? 0):\(library.features.artworkGeneration)") {
            guard let track else {
                image = nil
                return
            }
            image = await library.features.artwork(track)
        }
        .accessibilityHidden(true)
    }
}

struct Tones: Equatable {
    static let fallback = Tones(first: Color.accentColor.opacity(0.35), second: Color.clear)

    let first: Color
    let second: Color
}

enum Backdrop {
    private static let sampleEdge = 16
    private static let darkest: CGFloat = 0.12
    private static let dullest: CGFloat = 0.12

    static func tones(of image: UIImage) async -> Tones {
        await Task.detached(priority: .utility) { sampled(image) ?? .fallback }.value
    }

    private static func sampled(_ image: UIImage) -> Tones? {
        guard let source = image.cgImage else { return nil }
        let edge = sampleEdge
        var pixels = [UInt8](repeating: 0, count: edge * edge * 4)
        let drawn = pixels.withUnsafeMutableBytes { buffer -> Bool in
            guard let context = CGContext(
                data: buffer.baseAddress,
                width: edge,
                height: edge,
                bitsPerComponent: 8,
                bytesPerRow: edge * 4,
                space: CGColorSpaceCreateDeviceRGB(),
                bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
            ) else { return false }
            context.interpolationQuality = .medium
            context.draw(source, in: CGRect(x: 0, y: 0, width: edge, height: edge))
            return true
        }
        guard drawn else { return nil }
        func average(rows: Range<Int>) -> UIColor? {
            var red: CGFloat = 0, green: CGFloat = 0, blue: CGFloat = 0
            var count: CGFloat = 0
            for row in rows {
                for column in 0..<edge {
                    let at = (row * edge + column) * 4
                    red += CGFloat(pixels[at]) / 255
                    green += CGFloat(pixels[at + 1]) / 255
                    blue += CGFloat(pixels[at + 2]) / 255
                    count += 1
                }
            }
            let color = UIColor(red: red / count, green: green / count, blue: blue / count, alpha: 1)
            var hue: CGFloat = 0, saturation: CGFloat = 0, brightness: CGFloat = 0, alpha: CGFloat = 0
            color.getHue(&hue, saturation: &saturation, brightness: &brightness, alpha: &alpha)
            guard brightness > darkest, saturation > dullest else { return nil }
            return UIColor(hue: hue, saturation: min(saturation * 1.2, 1), brightness: max(brightness, 0.45), alpha: 1)
        }
        guard let top = average(rows: 0..<edge / 2) else { return nil }
        let bottom = average(rows: edge / 2..<edge) ?? top
        return Tones(first: Color(top).opacity(0.55), second: Color(bottom).opacity(0.35))
    }
}

struct BackdropView: View {
    @Environment(Library.self) private var library
    let track: Track?
    @State private var tones = Tones.fallback

    var body: some View {
        ZStack {
            Color(.systemBackground)
            RadialGradient(colors: [tones.first, .clear], center: .topLeading, startRadius: 0, endRadius: 600)
            RadialGradient(colors: [tones.second, .clear], center: .bottomTrailing, startRadius: 0, endRadius: 600)
        }
        .ignoresSafeArea()
        .animation(.easeInOut(duration: 0.6), value: tones)
        .task(id: "\(track?.id ?? 0):\(track?.artRevision ?? 0)") {
            guard let track, let image = await library.features.artwork(track) else {
                tones = .fallback
                return
            }
            tones = await Backdrop.tones(of: image)
        }
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
