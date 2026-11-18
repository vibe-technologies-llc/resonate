import ResonateLogic
import SwiftUI

@MainActor
@Observable
final class ScrollTrack {
    nonisolated private static let steps = 400.0

    private(set) var scrolled = 0.0
    private(set) var visible = false
    private(set) var grabbed = false
    @ObservationIgnored private var hiding: Task<Void, Never>?

    nonisolated static func share(of geometry: ScrollGeometry) -> Double {
        let insets = geometry.contentInsets
        let scrolled = FastScroll.scrolled(
            offset: geometry.contentOffset.y + insets.top,
            content: geometry.contentSize.height,
            container: geometry.containerSize.height - insets.top - insets.bottom
        )
        return (scrolled * steps).rounded() / steps
    }

    func moved(to share: Double) {
        if scrolled != share { scrolled = share }
        linger()
    }

    func grab() {
        if !grabbed { grabbed = true }
        linger()
    }

    func release() {
        if grabbed { grabbed = false }
        linger()
    }

    private func linger() {
        if !visible { visible = true }
        hiding?.cancel()
        hiding = Task { [weak self] in
            try? await Task.sleep(for: .seconds(FastScroll.lingersFor))
            guard !Task.isCancelled, let self, !self.grabbed else { return }
            self.visible = false
        }
    }
}

struct FastScroller: View {
    private static let fromTheEdge: CGFloat = 4
    private static let thumbWidth: CGFloat = 5
    private static let grabbedWidth: CGFloat = 8
    private static let reach: CGFloat = 24

    @Environment(\.accessibilityVoiceOverEnabled) private var voiceOver
    let track: ScrollTrack
    let rows: Int
    let window: Range<Int>
    let jump: (Int) -> Void
    @GestureState private var touched = false
    @State private var grabbedAt: Double?
    @State private var held: Double?
    @State private var asked: Int?

    var body: some View {
        if FastScroll.offered(rows: rows) {
            GeometryReader { space in
                let length = Double(space.size.height)
                let share = held ?? FastScroll.share(of: window, in: rows, scrolled: track.scrolled)
                let shown = track.visible || voiceOver
                Capsule()
                    .fill(Color.worn)
                    .frame(width: track.grabbed ? Self.grabbedWidth : Self.thumbWidth, height: FastScroll.thumbLength)
                    .motion(Flip.animation, value: track.grabbed)
                    .frame(width: Self.reach, height: FastScroll.thumbLength, alignment: .trailing)
                    .contentShape(.rect)
                    .accessibilityElement()
                    .accessibilityLabel("library.scroller")
                    .accessibilityValue(said("library.scroller.place", FastScroll.row(at: share, of: rows) + 1, rows))
                    .accessibilityAdjustableAction { direction in
                        go(to: FastScroll.adjusted(share, forward: direction == .increment))
                        letGo()
                    }
                    .gesture(
                        DragGesture(minimumDistance: 0)
                            .updating($touched) { _, touched, _ in touched = true }
                            .onChanged { drag in
                                let start = grabbedAt ?? share
                                let dragged = Double(drag.translation.height)
                                if grabbedAt == nil {
                                    grabbedAt = start
                                    track.grab()
                                }
                                guard held != nil || FastScroll.moved(by: dragged) else { return }
                                go(to: FastScroll.share(draggedFrom: start, by: dragged, track: length))
                            }
                    )
                    .onChange(of: touched) { _, touching in
                        if !touching { letGo() }
                    }
                    .offset(y: FastScroll.thumbOffset(share: share, track: length))
                    .frame(maxWidth: .infinity, alignment: .topTrailing)
                    .opacity(shown ? 1 : 0)
                    .motion(.easeOut(duration: shown ? FastScroll.fadesInOver : FastScroll.fadesOutOver), calm: .easeOut(duration: FastScroll.fadesInOver), value: shown)
                    .allowsHitTesting(shown)
            }
            .padding(.trailing, Self.fromTheEdge)
        }
    }

    private func go(to share: Double) {
        let row = FastScroll.row(at: share, of: rows)
        held = share
        guard asked != row else { return }
        asked = row
        jump(row)
    }

    private func letGo() {
        grabbedAt = nil
        held = nil
        asked = nil
        track.release()
    }
}
