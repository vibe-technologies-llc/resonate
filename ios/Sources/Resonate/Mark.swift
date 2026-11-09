import SwiftUI

struct ResonateMark: Shape {
    private struct Bar {
        let x: CGFloat
        let top: CGFloat
        let bottom: CGFloat
    }

    private static let viewport: CGFloat = 24
    private static let barWidth: CGFloat = 2
    private static let bars = [
        Bar(x: 4, top: 10, bottom: 14),
        Bar(x: 8, top: 6, bottom: 18),
        Bar(x: 12, top: 3.5, bottom: 20.5),
        Bar(x: 16, top: 7.5, bottom: 16.5),
        Bar(x: 20, top: 10, bottom: 14),
    ]

    func path(in rect: CGRect) -> Path {
        let unit = min(rect.width, rect.height) / Self.viewport
        let side = Self.viewport * unit
        let left = rect.midX - side / 2
        let top = rect.midY - side / 2
        let width = Self.barWidth * unit
        var path = Path()
        for bar in Self.bars {
            path.addRoundedRect(
                in: CGRect(
                    x: left + bar.x * unit - width / 2,
                    y: top + bar.top * unit - width / 2,
                    width: width,
                    height: (bar.bottom - bar.top) * unit + width
                ),
                cornerSize: CGSize(width: width / 2, height: width / 2)
            )
        }
        return path
    }
}
