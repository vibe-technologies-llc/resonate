import AVFoundation
import Foundation
import ResonateLogic
import ResonateNative
import Security

struct RouteReading: Equatable, Sendable {
    struct Output: Equatable, Sendable {
        let id: String
        let name: String
        let channels: Int
    }

    let outputs: [Output]
    let sampleRate: Double
    let latencyMs: Double

    static let unread = RouteReading(outputs: [], sampleRate: 0, latencyMs: 0)

    nonisolated static var current: RouteReading {
        let session = AVAudioSession.sharedInstance()
        return RouteReading(
            outputs: session.currentRoute.outputs.map { Output(id: $0.uid, name: $0.portName, channels: $0.channels?.count ?? 2) },
            sampleRate: session.sampleRate,
            latencyMs: (session.outputLatency + session.ioBufferDuration) * 1000
        )
    }
}

extension OutputRoute {
    static var current: OutputRoute? {
        AVAudioSession.sharedInstance().currentRoute.outputs.first.map { OutputRoute(id: $0.uid, name: $0.portName) }
    }
}

struct SecurityItems: SecretItems {
    private static let service = "org.resonate.accounts"

    private func query(_ account: String) -> [String: Any] {
        [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: Self.service,
            kSecAttrAccount as String: account,
        ]
    }

    func copy(_ account: String) -> (status: Int32, data: Data?) {
        var asked = query(account)
        asked[kSecReturnData as String] = true
        asked[kSecMatchLimit as String] = kSecMatchLimitOne
        var found: CFTypeRef?
        let status = SecItemCopyMatching(asked as CFDictionary, &found)
        return (status, found as? Data)
    }

    func update(_ account: String, _ data: Data) -> Int32 {
        SecItemUpdate(query(account) as CFDictionary, [kSecValueData as String: data] as CFDictionary)
    }

    func add(_ account: String, _ data: Data) -> Int32 {
        var added = query(account)
        added[kSecValueData as String] = data
        added[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        return SecItemAdd(added as CFDictionary, nil)
    }

    func delete(_ account: String) -> Int32 {
        SecItemDelete(query(account) as CFDictionary)
    }
}

extension Keychain {
    init() {
        self.init(items: SecurityItems())
    }
}
