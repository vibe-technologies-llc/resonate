import ResonateNative
import SwiftUI

extension MixAccent {
    var color: Color {
        switch self {
        case .mauve: .purple
        case .blue: .blue
        case .teal: .teal
        case .green: .green
        case .amber: .yellow
        case .peach: .orange
        case .red: .red
        }
    }

    var ink: Color {
        switch self {
        case .mauve, .blue, .red: .white
        case .teal, .green, .amber, .peach: .black.opacity(0.78)
        }
    }
}

struct MosaicArtwork: View {
    private static let tilesASide = 2
    private static let markShare: CGFloat = 0.22
    private static let nameShare: CGFloat = 0.11
    private static let insetShare: CGFloat = 0.08
    private static let nameAlpha = 0.62
    private static let letteredFrom: CGFloat = 100

    let covers: [Track]
    let accents: MixAccents
    let mark: String
    let name: String
    var corner: CGFloat = 12

    var body: some View {
        GeometryReader { proxy in
            drawn(side: min(proxy.size.width, proxy.size.height))
        }
        .aspectRatio(1, contentMode: .fit)
        .clipShape(.rect(cornerRadius: corner, style: .continuous))
        .overlay {
            RoundedRectangle(cornerRadius: corner, style: .continuous)
                .strokeBorder(.primary.opacity(0.06), lineWidth: 1)
        }
        .accessibilityHidden(true)
    }

    @ViewBuilder
    private func drawn(side: CGFloat) -> some View {
        switch covers.count {
        case 0:
            lettered(side: side)
        case 1:
            ArtworkView(track: covers[0], corner: 0)
                .frame(width: side, height: side)
        default:
            let tile = side / CGFloat(Self.tilesASide)
            Grid(horizontalSpacing: 0, verticalSpacing: 0) {
                ForEach(0..<Self.tilesASide, id: \.self) { row in
                    GridRow {
                        ForEach(0..<Self.tilesASide, id: \.self) { column in
                            let at = row * Self.tilesASide + column
                            if at < covers.count {
                                ArtworkView(track: covers[at], corner: 0).frame(width: tile, height: tile)
                            } else {
                                Rectangle()
                                    .fill(at.isMultiple(of: 2) ? accents.from.color : accents.to.color)
                                    .frame(width: tile, height: tile)
                            }
                        }
                    }
                }
            }
        }
    }

    private func lettered(side: CGFloat) -> some View {
        ZStack(alignment: .topLeading) {
            LinearGradient(
                colors: [accents.from.color, accents.to.color],
                startPoint: .topLeading,
                endPoint: .bottomTrailing
            )
            VStack(alignment: .leading, spacing: 0) {
                Image(systemName: mark)
                    .font(.system(size: side * Self.markShare, weight: .semibold))
                Spacer(minLength: 0)
                if side >= Self.letteredFrom, !name.isEmpty {
                    Text(name)
                        .font(.system(size: side * Self.nameShare, weight: .bold))
                        .lineLimit(3)
                        .opacity(Self.nameAlpha)
                }
            }
            .foregroundStyle(accents.from.ink)
            .padding(side * Self.insetShare)
            .frame(width: side, height: side, alignment: .topLeading)
        }
        .frame(width: side, height: side)
    }
}
