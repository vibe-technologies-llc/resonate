import Foundation

enum Backup {
    private static let derivedFolders = ["sync-received", "sync-staging"]
    private static let keptCopyMarker = ".before-v"

    static func leaveOutDerived(in support: URL) {
        let files = FileManager.default
        try? files.createDirectory(at: support, withIntermediateDirectories: true)
        for name in derivedFolders {
            let folder = support.appendingPathComponent(name, isDirectory: true)
            try? files.createDirectory(at: folder, withIntermediateDirectories: true)
            leaveOut(folder)
        }
        let names = (try? files.contentsOfDirectory(atPath: support.path)) ?? []
        for name in names where name.contains(keptCopyMarker) {
            leaveOut(support.appendingPathComponent(name))
        }
    }

    private static func leaveOut(_ url: URL) {
        var target = url
        var values = URLResourceValues()
        values.isExcludedFromBackup = true
        try? target.setResourceValues(values)
    }
}
