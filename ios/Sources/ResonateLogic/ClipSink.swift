import Foundation
import Synchronization

public final class ClipSink: Sendable {
    private struct Held {
        var samples: [Float] = []
        var waiting: CheckedContinuation<[Float], any Swift.Error>?
        var done = false
        var stopped = false
        var interrupted = false
    }

    private let wanted: Int
    private let held: Mutex<Held>

    public init(wanted: Int) {
        self.wanted = wanted
        var empty = Held()
        empty.samples.reserveCapacity(wanted)
        held = Mutex(empty)
    }

    public func interrupt() {
        let waiting = held.withLock { held -> CheckedContinuation<[Float], any Swift.Error>? in
            guard !held.done else { return nil }
            held.done = true
            held.interrupted = true
            defer { held.waiting = nil }
            return held.waiting
        }
        waiting?.resume(throwing: MicrophoneFailure.interrupted)
    }

    public func finish() {
        let ready = held.withLock { held -> (CheckedContinuation<[Float], any Swift.Error>, [Float])? in
            guard !held.done else { return nil }
            held.done = true
            defer { held.waiting = nil }
            return held.waiting.map { ($0, held.samples) }
        }
        if let (waiting, samples) = ready { waiting.resume(returning: samples) }
    }

    public func filled() async throws -> [Float] {
        try await withTaskCancellationHandler {
            try await withCheckedThrowingContinuation { continuation in
                let ready = held.withLock { held -> Result<[Float], any Swift.Error>? in
                    if held.stopped { return .failure(CancellationError()) }
                    if held.interrupted { return .failure(MicrophoneFailure.interrupted) }
                    if held.done { return .success(held.samples) }
                    held.waiting = continuation
                    return nil
                }
                if let ready { continuation.resume(with: ready) }
            }
        } onCancel: {
            let waiting = held.withLock { held -> CheckedContinuation<[Float], any Swift.Error>? in
                held.stopped = true
                held.done = true
                defer { held.waiting = nil }
                return held.waiting
            }
            waiting?.resume(throwing: CancellationError())
        }
    }

    public func append(_ mono: [Float], sharing share: Float) {
        let ready = held.withLock { held -> (CheckedContinuation<[Float], any Swift.Error>, [Float])? in
            guard !held.done else { return nil }
            let room = wanted - held.samples.count
            held.samples.append(contentsOf: mono.prefix(room).map { $0 * share })
            guard held.samples.count >= wanted else { return nil }
            held.done = true
            defer { held.waiting = nil }
            return held.waiting.map { ($0, held.samples) }
        }
        if let (waiting, samples) = ready { waiting.resume(returning: samples) }
    }
}
