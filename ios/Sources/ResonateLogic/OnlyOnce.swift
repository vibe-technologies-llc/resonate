import Synchronization

public final class OnlyOnce<Value: Sendable>: Sendable {
    private struct Held: Sendable {
        var done = false
        var early: Value?
        var waiting: CheckedContinuation<Value, Never>?
    }

    private let held = Mutex(Held())

    public init() {}

    public func give(_ value: Value) {
        let waiting = held.withLock { held -> CheckedContinuation<Value, Never>? in
            guard !held.done else { return nil }
            held.done = true
            guard let waiting = held.waiting else {
                held.early = value
                return nil
            }
            held.waiting = nil
            return waiting
        }
        waiting?.resume(returning: value)
    }

    public func value() async -> Value {
        await withCheckedContinuation { continuation in
            let ready = held.withLock { held -> Value? in
                if let early = held.early { return early }
                held.waiting = continuation
                return nil
            }
            if let ready { continuation.resume(returning: ready) }
        }
    }
}
