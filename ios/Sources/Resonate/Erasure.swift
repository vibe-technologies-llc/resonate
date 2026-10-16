import Foundation
import Security

struct ErasureLeftovers: Equatable {
    var songs = 0
    var files = 0
    var secrets = 0

    var isNone: Bool {
        songs + files + secrets == 0
    }

    var sentences: [String] {
        var told: [String] = []
        if songs > 0 {
            told.append("\(counted(songs, "song")) could not be deleted and stay in their folders, which Resonate keeps.")
        }
        if files > 0 {
            told.append("\(counted(files, "file")) of the app's own could not be deleted.")
        }
        if secrets > 0 {
            told.append("\(counted(secrets, "saved key")) could not be deleted.")
        }
        return told
    }
}

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

    static func everything(keeping names: [String] = []) -> ErasureLeftovers {
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

        var leftovers = ErasureLeftovers()
        leftovers.files = places.reduce(0) { $0 + emptied($1) }

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

        leftovers.secrets = keychainClasses.filter { !deleted($0) }.count
        return leftovers
    }

    private static func deleted(_ kind: CFString) -> Bool {
        let status = SecItemDelete([kSecClass as String: kind] as CFDictionary)
        return status == errSecSuccess || status == errSecItemNotFound
    }

    private static func emptied(_ folder: URL) -> Int {
        let files = FileManager.default
        guard files.fileExists(atPath: folder.path) else { return 0 }
        guard let contents = try? files.contentsOfDirectory(at: folder, includingPropertiesForKeys: nil) else { return 1 }
        return contents.filter { item in
            do {
                try files.removeItem(at: item)
                return false
            } catch {
                return files.fileExists(atPath: item.path)
            }
        }.count
    }
}
