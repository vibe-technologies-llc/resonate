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

    static func everything(keeping names: [String] = []) {
        let files = FileManager.default
        let support = Library.supportFolder
        let kept = names.compactMap { name in
            (try? Data(contentsOf: support.appendingPathComponent(name))).map { (name, $0) }
        }
        let places = [
            files.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0],
            files.urls(for: .cachesDirectory, in: .userDomainMask)[0],
            files.urls(for: .documentDirectory, in: .userDomainMask)[0],
            files.temporaryDirectory,
        ]
        places.forEach(emptied)

        if !kept.isEmpty {
            try? files.createDirectory(at: support, withIntermediateDirectories: true)
            for (name, data) in kept { try? data.write(to: support.appendingPathComponent(name), options: .atomic) }
        }

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
