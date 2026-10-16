import QuartzCore
import SwiftUI

@MainActor
final class FullFrameRate: NSObject {
    static let shared = FullFrameRate()

    static let fullest = 120
    static let spared = 60

    private static let lowestShare: Float = 2.0 / 3.0

    private var holders = 0
    private var link: CADisplayLink?

    static func range(at rate: Float) -> CAFrameRateRange {
        CAFrameRateRange(minimum: rate * lowestShare, maximum: rate, preferred: rate)
    }

    func hold() {
        holders += 1
        guard link == nil else { return }
        let started = CADisplayLink(target: self, selector: #selector(tick))
        started.preferredFrameRateRange = Self.range(at: Float(Self.fullest))
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
