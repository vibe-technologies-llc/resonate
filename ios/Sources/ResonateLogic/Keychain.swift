import Foundation

public enum KeychainStatus {
    public static let success: Int32 = 0
    public static let itemNotFound: Int32 = -25300
}

public enum KeychainFailure: Swift.Error, LocalizedError {
    case refused(Int32)
    case unread(Int32)

    public var errorDescription: String? {
        switch self {
        case .refused(let status): "The keychain refused the secret (\(status))"
        case .unread(let status): "The keychain could not be read (\(status))"
        }
    }
}

public protocol SecretItems: Sendable {
    func copy(_ account: String) -> (status: Int32, data: Data?)
    func update(_ account: String, _ data: Data) -> Int32
    func add(_ account: String, _ data: Data) -> Int32
    func delete(_ account: String) -> Int32
}

public protocol Secrets: Sendable {
    func read(_ name: String) throws -> String
    func write(_ name: String, _ value: String) throws
}

public struct Keychain: Secrets {
    private let items: any SecretItems

    public init(items: any SecretItems) {
        self.items = items
    }

    public func read(_ name: String) throws -> String {
        let (status, data) = items.copy(name)
        if status == KeychainStatus.itemNotFound { return "" }
        guard status == KeychainStatus.success, let data else { throw KeychainFailure.unread(status) }
        return String(decoding: data, as: UTF8.self)
    }

    public func write(_ name: String, _ value: String) throws {
        guard !value.isEmpty else {
            let removed = items.delete(name)
            guard removed == KeychainStatus.success || removed == KeychainStatus.itemNotFound else {
                throw KeychainFailure.refused(removed)
            }
            return
        }
        let data = Data(value.utf8)
        let updated = items.update(name, data)
        if updated == KeychainStatus.success { return }
        guard updated == KeychainStatus.itemNotFound else { throw KeychainFailure.refused(updated) }

        let status = items.add(name, data)
        guard status == KeychainStatus.success else { throw KeychainFailure.refused(status) }
    }
}
