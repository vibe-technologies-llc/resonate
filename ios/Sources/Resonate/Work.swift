import Foundation
import ResonateNative

enum Work {
    private static let orderedQueue = DispatchQueue(
        label: "org.resonate.ordered",
        qos: .userInitiated
    )
    private static let libraryQueue = DispatchQueue(
        label: "org.resonate.library",
        qos: .userInitiated
    )
    private static let syncQueue = DispatchQueue(
        label: "org.resonate.sync",
        qos: .userInitiated
    )
    private static let sessionQueue = DispatchQueue(
        label: "org.resonate.session",
        qos: .userInitiated
    )
    private static let secretsQueue = DispatchQueue(
        label: "org.resonate.secrets",
        qos: .userInitiated
    )
    private static let asideQueue = DispatchQueue(
        label: "org.resonate.aside",
        qos: .utility,
        attributes: .concurrent
    )

    static func inOrder<Value: Sendable>(
        _ body: @escaping @Sendable () throws -> Value
    ) async throws -> Value {
        try await withCheckedThrowingContinuation { continuation in
            orderedQueue.async { continuation.resume(with: Result { try body() }) }
        }
    }

    static func enqueueInOrder(_ body: @escaping @Sendable () -> Void) {
        orderedQueue.async(execute: body)
    }

    static func inSession<Value: Sendable>(
        _ body: @escaping @Sendable () -> Value
    ) async -> Value {
        await withCheckedContinuation { continuation in
            sessionQueue.async { continuation.resume(returning: body()) }
        }
    }

    static func inLibrary<Value: Sendable>(
        _ body: @escaping @Sendable () throws -> Value
    ) async throws -> Value {
        try await withCheckedThrowingContinuation { continuation in
            libraryQueue.async { continuation.resume(with: Result { try body() }) }
        }
    }

    static func inSync<Value: Sendable>(
        _ body: @escaping @Sendable () throws -> Value
    ) async throws -> Value {
        try await withCheckedThrowingContinuation { continuation in
            syncQueue.async { continuation.resume(with: Result { try body() }) }
        }
    }

    static func inSecrets<Value: Sendable>(
        _ body: @escaping @Sendable () throws -> Value
    ) async throws -> Value {
        try await withCheckedThrowingContinuation { continuation in
            secretsQueue.async { continuation.resume(with: Result { try body() }) }
        }
    }

    static func aside<Value: Sendable>(
        _ body: @escaping @Sendable () throws -> Value
    ) async throws -> Value {
        try await withCheckedThrowingContinuation { continuation in
            asideQueue.async { continuation.resume(with: Result { try body() }) }
        }
    }
}

func explained(_ failure: any Swift.Error) -> String {
    if let native = failure as? ResonateNative.Error,
        let reason = Mirror(reflecting: native).children.first
    {
        if let message = reason.value as? String {
            return message
        }
        if let message = Mirror(reflecting: reason.value).children.first?.value as? String {
            return message
        }
    }
    return failure.localizedDescription
}

func counted(_ count: Int, _ noun: String) -> String {
    "\(count) \(count == 1 ? noun : noun + "s")"
}

func clock(_ milliseconds: UInt64) -> String {
    let seconds = milliseconds / 1000
    let hours = seconds / 3600
    let minutes = seconds / 60 % 60
    let rest = seconds % 60
    return hours > 0
        ? String(format: "%d:%02d:%02d", hours, minutes, rest)
        : String(format: "%d:%02d", minutes, rest)
}
