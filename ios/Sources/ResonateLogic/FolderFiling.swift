import Foundation
import ResonateNative

public struct FiledMoves: Sendable {
    public var moved: [MovedDocument] = []
    public var back: [FiledSong] = []
    public var failed = 0

    public init() {}
}

public enum FolderURI {
    public static let scheme = "resonate-folder"

    public static func make(root: String, relative: String) -> String {
        let escaped = relative.split(separator: "/").map {
            String($0).addingPercentEncoding(withAllowedCharacters: .urlPathAllowed.subtracting(["/", "%", "?", "#"])) ?? String($0)
        }
        return "\(scheme)://\(root)/\(escaped.joined(separator: "/"))"
    }

    public static func root(of uri: String) -> String? {
        let opening = "\(scheme)://"
        guard uri.hasPrefix(opening) else { return nil }
        return uri.dropFirst(opening.count).split(separator: "/", maxSplits: 1).first.map(String.init)
    }

    public static func split(_ uri: String) -> (root: String, relative: String)? {
        let prefix = "\(scheme)://"
        guard uri.hasPrefix(prefix) else { return nil }
        let rest = uri.dropFirst(prefix.count)
        return rest.firstIndex(of: "/").map { (String(rest[..<$0]), String(rest[rest.index(after: $0)...])) }
            ?? (String(rest), "")
    }

    public static func components(of relative: String) -> [String]? {
        let path = relative.split(separator: "/").map { $0.removingPercentEncoding ?? String($0) }
        return path.contains("..") ? nil : path
    }
}

public struct ResolvedDocument {
    public let url: URL
    public let release: () -> Void

    public init(url: URL, release: @escaping () -> Void) {
        self.url = url
        self.release = release
    }
}

public enum FolderFiling {
    public static let sidecarExtension = "lrc"

    public static func file(
        _ moves: [FiledSong],
        in id: String,
        at root: URL,
        resolving resolve: (String) throws -> ResolvedDocument
    ) -> FiledMoves {
        let manager = FileManager.default
        let rootPath = root.standardizedFileURL.path
        var filed = FiledMoves()
        var left: Set<URL> = []
        for song in moves {
            guard let source = try? resolve(song.from) else {
                filed.failed += 1
                continue
            }
            defer { source.release() }
            let from = source.url
            let to = song.to.reduce(root) { $0.appendingPathComponent($1) }
            let fromPath = from.standardizedFileURL.path
            guard fromPath.hasPrefix(rootPath + "/"), !manager.fileExists(atPath: to.path) else {
                filed.failed += 1
                continue
            }
            do {
                try manager.createDirectory(at: to.deletingLastPathComponent(), withIntermediateDirectories: true)
                try manager.moveItem(at: from, to: to)
            } catch {
                filed.failed += 1
                continue
            }
            let sidecar = from.deletingPathExtension().appendingPathExtension(sidecarExtension)
            let sidecarTo = to.deletingPathExtension().appendingPathExtension(sidecarExtension)
            if manager.fileExists(atPath: sidecar.path), !manager.fileExists(atPath: sidecarTo.path) {
                try? manager.moveItem(at: sidecar, to: sidecarTo)
            }
            let placed = FolderURI.make(root: id, relative: song.to.joined(separator: "/"))
            let before = fromPath.dropFirst(rootPath.count + 1).split(separator: "/").map(String.init)
            filed.moved.append(MovedDocument(id: song.id, uri: placed))
            filed.back.append(FiledSong(id: song.id, from: placed, to: before))
            left.insert(from.deletingLastPathComponent().standardizedFileURL)
        }
        for folder in left { prune(folder, above: rootPath) }
        return filed
    }

    private static func prune(_ folder: URL, above rootPath: String) {
        let manager = FileManager.default
        var standing = folder
        while standing.path.hasPrefix(rootPath + "/"),
            let held = try? manager.contentsOfDirectory(atPath: standing.path),
            held.allSatisfy({ $0 == ".DS_Store" })
        {
            try? manager.removeItem(at: standing)
            standing = standing.deletingLastPathComponent()
        }
    }
}
