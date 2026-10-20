import Foundation

public actor DecodeSlots {
    private struct Waiter {
        let id: UInt64
        let continuation: CheckedContinuation<Void, any Swift.Error>
    }

    private var free: Int
    private var waiters: [Waiter] = []
    private var issued: UInt64 = 0

    public init(_ count: Int) {
        free = count
    }

    public func acquire() async throws {
        try Task.checkCancellation()
        if free > 0 {
            free -= 1
            return
        }
        issued += 1
        let id = issued
        try await withTaskCancellationHandler {
            try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, any Swift.Error>) in
                if Task.isCancelled {
                    continuation.resume(throwing: CancellationError())
                } else {
                    waiters.append(Waiter(id: id, continuation: continuation))
                }
            }
        } onCancel: {
            Task { await self.abandon(id) }
        }
    }

    public func release() {
        if waiters.isEmpty {
            free += 1
        } else {
            waiters.removeFirst().continuation.resume()
        }
    }

    private func abandon(_ id: UInt64) {
        guard let index = waiters.firstIndex(where: { $0.id == id }) else { return }
        waiters.remove(at: index).continuation.resume(throwing: CancellationError())
    }
}


public struct Flights<Value: Sendable> {
    private struct Flight {
        let task: Task<Value?, Never>
        var waiting: Int
    }

    private var flights: [ArtworkRequest: Flight] = [:]

    public init() {}

    public var count: Int { flights.count }

    public mutating func join(
        _ request: ArtworkRequest,
        starting: () -> Task<Value?, Never>
    ) -> Task<Value?, Never> {
        let flight = flights[request] ?? Flight(task: starting(), waiting: 0)
        flights[request] = Flight(task: flight.task, waiting: flight.waiting + 1)
        return flight.task
    }

    public mutating func leave(_ request: ArtworkRequest, _ task: Task<Value?, Never>) {
        guard var flight = flights[request], flight.task == task else { return }
        flight.waiting -= 1
        if flight.waiting > 0 {
            flights[request] = flight
        } else {
            flights[request] = nil
            task.cancel()
        }
    }

    public mutating func settle(_ request: ArtworkRequest, _ task: Task<Value?, Never>) {
        if flights[request]?.task == task { flights[request] = nil }
    }
}
