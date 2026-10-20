import Foundation

public struct WrittenPosition: Sendable {
    public let positionMs: UInt64
    public let at: ContinuousClock.Instant
    public let rate: Double

    public init(positionMs: UInt64, at: ContinuousClock.Instant, rate: Double) {
        self.positionMs = positionMs
        self.at = at
        self.rate = rate
    }

    public func expected(at now: ContinuousClock.Instant) -> Double {
        let elapsed = now - at
        let elapsedMs = Double(elapsed.components.seconds) * 1000 + Double(elapsed.components.attoseconds) * 1e-15
        return Double(positionMs) + elapsedMs * rate
    }
}
