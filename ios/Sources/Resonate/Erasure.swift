import Foundation
import Security

enum Erasure {
    private static let suites = ["library-features", "playback", "provider-accounts", "appearance"]
    private static var keychainClasses: [CFString] {
        [
            kSecClassGenericPassword,
            kSecClassInternetPassword,
            kSecClassKey,
            kSecClassCertificate,
            kSecClassIdentity,
        ]
    }

    static func everything() {
        let files = FileManager.default
        let places = [
            files.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0],
            files.urls(for: .cachesDirectory, in: .userDomainMask)[0],
            files.urls(for: .documentDirectory, in: .userDomainMask)[0],
            files.temporaryDirectory,
        ]
        places.forEach(emptied)

        for suite in suites {
            let defaults = UserDefaults(suiteName: suite)
            defaults?.removePersistentDomain(forName: suite)
            defaults?.synchronize()
        }
        if let bundle = Bundle.main.bundleIdentifier {
            UserDefaults.standard.removePersistentDomain(forName: bundle)
            UserDefaults.standard.synchronize()
        }

        for kind in keychainClasses {
            SecItemDelete([kSecClass as String: kind] as CFDictionary)
        }
    }

    private static func emptied(_ folder: URL) {
        let files = FileManager.default
        guard let contents = try? files.contentsOfDirectory(at: folder, includingPropertiesForKeys: nil) else { return }
        for item in contents {
            try? files.removeItem(at: item)
        }
    }
}
