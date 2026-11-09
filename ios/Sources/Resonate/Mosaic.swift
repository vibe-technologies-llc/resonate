import ResonateNative
import SwiftUI

struct MosaicArtwork: View {
    private static let tilesASide = 2
    private static let markShare: CGFloat = 0.2
    private static let nameShare: CGFloat = 0.1
    private static let insetShare: CGFloat = 0.09
    private static let letteredFrom: CGFloat = 100
    private static let nameLines = 3

    let covers: [Track]
    let mark: String
    let name: String
    var corner: CGFloat = 12

    var body: some View {
        GeometryReader { proxy in
            drawn(side: min(proxy.size.width, proxy.size.height))
        }
        .aspectRatio(1, contentMode: .fit)
        .clipShape(.rect(cornerRadius: corner, style: .continuous))
        .accessibilityIgnoresInvertColors()
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
                                    .fill(at.isMultiple(of: 2) ? AnyShapeStyle(.quaternary) : AnyShapeStyle(.quinary))
                                    .frame(width: tile, height: tile)
                            }
                        }
                    }
                }
            }
        }
    }

    private func lettered(side: CGFloat) -> some View {
        let markSide = side * Self.markShare
        return VStack(alignment: .leading, spacing: 0) {
            Image(systemName: mark)
                .resizable()
                .scaledToFit()
                .frame(width: markSide, height: markSide, alignment: .topLeading)
                .foregroundStyle(.tertiary)
                .accessibilityHidden(true)
            Spacer(minLength: 0)
            if side >= Self.letteredFrom, !name.isEmpty {
                Text(name)
                    .font(.system(size: side * Self.nameShare, weight: .semibold))
                    .lineLimit(Self.nameLines)
                    .foregroundStyle(.secondary)
            }
        }
        .padding(side * Self.insetShare)
        .frame(width: side, height: side, alignment: .topLeading)
        .background(.quaternary)
    }
}
