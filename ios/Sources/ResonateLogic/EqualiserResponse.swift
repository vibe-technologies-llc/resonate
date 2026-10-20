import Foundation
import Observation
import ResonateNative

@MainActor
@Observable
public final class EqualiserResponse {
    public nonisolated static let points: UInt32 = 256
    public nonisolated static let between: Duration = .milliseconds(30)

    private struct Asked: Equatable {
        let curve: EqualiserCurve
        let rate: UInt32?
    }

    public private(set) var decibels: [Float] = []
    @ObservationIgnored private var asked: Asked?
    @ObservationIgnored private var drawn: Asked?
    @ObservationIgnored private var working = false
    @ObservationIgnored private let tracing: @Sendable (EqualiserCurve, UInt32?) async throws -> [Float]
    @ObservationIgnored private let between: Duration

    public init(
        tracing: @escaping @Sendable (EqualiserCurve, UInt32?) async throws -> [Float],
        between: Duration = EqualiserResponse.between
    ) {
        self.tracing = tracing
        self.between = between
    }

    public func follow(_ curve: EqualiserCurve, rate: UInt32?) {
        let next = Asked(curve: curve, rate: rate)
        if next == drawn, !working {
            asked = nil
            return
        }
        asked = next
        guard !working else { return }
        working = true
        Task { await drain() }
    }

    private func drain() async {
        while let next = asked {
            asked = nil
            let curve = next.curve
            let rate = next.rate
            if let traced = try? await tracing(curve, rate) {
                decibels = traced
                drawn = next
            }
            if asked != nil { try? await Task.sleep(for: between) }
        }
        working = false
    }
}
