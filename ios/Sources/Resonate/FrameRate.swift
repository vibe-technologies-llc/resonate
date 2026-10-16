import QuartzCore
import SwiftUI

@MainActor
final class FullFrameRate: NSObject {
    static let shared = FullFrameRate()

    private static let fullest = CAFrameRateRange(minimum: 80, maximum: 120, preferred: 120)

    private var holders = 0
    private var link: CADisplayLink?

    func hold() {
        holders += 1
        guard link == nil else { return }
        let started = CADisplayLink(target: self, selector: #selector(tick))
        started.preferredFrameRateRange = Self.fullest
        started.add(to: .main, forMode: .common)
        link = started
    }

    func release() {
        holders = max(holders - 1, 0)
        guard holders == 0 else { return }
        link?.invalidate()
        link = nil
    }

    @objc private func tick(_ link: CADisplayLink) {}
}

private struct FullFrameRateHold: ViewModifier {
    let active: Bool

    @State private var held = false

    func body(content: Content) -> some View {
        content
            .onChange(of: active, initial: true) { _, wanted in holding(wanted) }
            .onDisappear { holding(false) }
    }

    private func holding(_ wanted: Bool) {
        guard wanted != held else { return }
        held = wanted
        if wanted {
            FullFrameRate.shared.hold()
        } else {
            FullFrameRate.shared.release()
        }
    }
}

extension View {
    func fullFrameRate(while active: Bool) -> some View {
        modifier(FullFrameRateHold(active: active))
    }
}
