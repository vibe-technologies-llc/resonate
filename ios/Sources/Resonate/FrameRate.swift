import QuartzCore

@MainActor
final class FrameRateBoost: NSObject {
    static let shared = FrameRateBoost()

    private static let fullest = CAFrameRateRange(minimum: 80, maximum: 120, preferred: 120)

    private var link: CADisplayLink?
    private var until: CFTimeInterval = 0

    func boost(for seconds: CFTimeInterval) {
        until = max(until, CACurrentMediaTime() + seconds)
        guard link == nil else { return }
        let started = CADisplayLink(target: self, selector: #selector(tick))
        started.preferredFrameRateRange = Self.fullest
        started.add(to: .main, forMode: .common)
        link = started
    }

    @objc private func tick(_ link: CADisplayLink) {
        guard link.timestamp >= until else { return }
        link.invalidate()
        self.link = nil
    }
}
