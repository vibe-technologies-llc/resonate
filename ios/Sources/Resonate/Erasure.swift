import Foundation
import ResonateLogic
import Security

extension Erasure {
    private static var keychainClasses: [CFString] {
        [
            kSecClassGenericPassword,
            kSecClassInternetPassword,
            kSecClassKey,
            kSecClassCertificate,
            kSecClassIdentity,
        ]
    }

    static func everything(keeping names: [String] = []) -> ErasureLeftovers {
        let files = FileManager.default
        return everything(
            keeping: names,
            support: Library.supportFolder,
            places: [
                files.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0],
                files.urls(for: .cachesDirectory, in: .userDomainMask)[0],
                files.urls(for: .documentDirectory, in: .userDomainMask)[0],
                files.temporaryDirectory,
            ],
            bundle: Bundle.main.bundleIdentifier,
            secrets: { keychainClasses.filter { !deleted($0) }.count }
        )
    }

    private static func deleted(_ kind: CFString) -> Bool {
        let status = SecItemDelete([kSecClass as String: kind] as CFDictionary)
        return status == errSecSuccess || status == errSecItemNotFound
    }
}
