import Foundation

public struct ErasureLeftovers: Equatable, Sendable {
    public var songs = 0
    public var files = 0
    public var secrets = 0

    public init(songs: Int = 0, files: Int = 0, secrets: Int = 0) {
        self.songs = songs
        self.files = files
        self.secrets = secrets
    }

    public var isNone: Bool {
        songs + files + secrets == 0
    }

    public var sentences: [String] {
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

public enum Erasure {
    public static let suites = ["library-features", "playback", "provider-accounts", "appearance"]

    public static func everything(
        keeping names: [String] = [],
        support: URL,
        places: [URL],
        suites: [String] = Erasure.suites,
        bundle: String?,
        secrets: () -> Int
    ) -> ErasureLeftovers {
        let files = FileManager.default
        let kept = names.compactMap { name in
            (try? Data(contentsOf: support.appendingPathComponent(name))).map { (name, $0) }
        }

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
        if let bundle {
            UserDefaults.standard.removePersistentDomain(forName: bundle)
            UserDefaults.standard.synchronize()
        }

        leftovers.secrets = secrets()
        return leftovers
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
