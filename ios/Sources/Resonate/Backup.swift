import Foundation

enum Backup {
    private static let derivedFolders = ["sync-received", "sync-staging", "download-partials"]

    static func leaveOutDerived(in support: URL) {
        let files = FileManager.default
        try? files.createDirectory(at: support, withIntermediateDirectories: true)
        for name in derivedFolders {
            let folder = support.appendingPathComponent(name, isDirectory: true)
            try? files.createDirectory(at: folder, withIntermediateDirectories: true)
            leaveOut(folder)
        }
    }

    static func leaveOutLanded(_ uri: String, from folders: Folders) {
        guard let url = folders.localFile(uri) else { return }
        leaveOut(url)
    }

    private static func leaveOut(_ url: URL) {
        var target = url
        var values = URLResourceValues()
        values.isExcludedFromBackup = true
        try? target.setResourceValues(values)
    }
}
