import Foundation
import ResonateNative

enum Work {
    private static let orderedQueue = DispatchQueue(
        label: "org.resonate.ordered",
        qos: .userInitiated
    )
    private static let savingQueue = DispatchQueue(
        label: "org.resonate.saving",
        qos: .utility
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
    private static let onScreenQueue = DispatchQueue(
        label: "org.resonate.on-screen",
        qos: .userInitiated,
        attributes: .concurrent
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

    static func inSaving(_ body: @escaping @Sendable () throws -> Void) async throws {
        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Swift.Error>) in
            savingQueue.async { continuation.resume(with: Result { try body() }) }
        }
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

    static func onScreen<Value: Sendable>(
        _ body: @escaping @Sendable () throws -> Value
    ) async throws -> Value {
        try await withCheckedThrowingContinuation { continuation in
            onScreenQueue.async { continuation.resume(with: Result { try body() }) }
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
