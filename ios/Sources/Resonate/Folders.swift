import Darwin
import Foundation
import ResonateNative
import Synchronization

struct MusicFolder: Identifiable, Hashable, Sendable {
    let id: String
    let name: String
    let path: String
    let readable: Bool
    let writable: Bool
}

struct FolderAccess: Sendable {
    static let unreachable = FolderAccess(path: "", readable: false, writable: false)

    let path: String
    let readable: Bool
    let writable: Bool

    static func of(_ url: URL) -> FolderAccess {
        let files = FileManager.default
        let values = try? url.resourceValues(forKeys: [.isReadableKey, .isWritableKey])
        return FolderAccess(
            path: url.resolvingSymlinksInPath().standardizedFileURL.path,
            readable: files.isReadableFile(atPath: url.path) && values?.isReadable != false,
            writable: files.isWritableFile(atPath: url.path) && values?.isWritable != false
        )
    }
}

final class ScanStop: Sendable {
    private let stopped = Atomic<Bool>(false)

    var isStopped: Bool { stopped.load(ordering: .relaxed) }

    func stop() {
        stopped.store(true, ordering: .relaxed)
    }
}

struct FoundDocument: Sendable {
    let uri: String
    let name: String
    let cover: String?
    let lyrics: String?
}

enum FolderFailure: Swift.Error, LocalizedError {
    case unknownFolder(String)
    case notAFolderUri(String)
    case refused(String)
    case alreadyAdded(String)

    var errorDescription: String? {
        switch self {
        case .unknownFolder: "That music folder is no longer available"
        case .notAFolderUri: "That song is not inside a chosen music folder"
        case .refused(let name): "\(name) could not be created"
        case .alreadyAdded: "This folder is already in your library"
        }
    }
}

final class Folders: Documents, @unchecked Sendable {
    static let scheme = "resonate-folder"
    static let local = "local"
    private static let deepest = 32
    private static let coverNames = ["cover.jpg", "cover.png", "folder.jpg", "folder.png", "front.jpg", "front.png"]
    private static let sidecarLimit = 1024 * 1024
    private static let coverLimit = 8 * 1024 * 1024

    private let lock = NSLock()
    private let bookmarks: URL
    private var roots: [String: Data] = [:]
    private var names: [String: String] = [:]
    private var access: [String: FolderAccess] = [:]
    private var scans: [String: ScanStop] = [:]
    private let extensions: Set<String>

    init(support: URL) {
        bookmarks = support.appendingPathComponent("folders.plist")
        extensions = Set(audioExtensions())
        if let data = try? Data(contentsOf: bookmarks),
            let stored = try? PropertyListDecoder().decode([String: [Data]].self, from: data)
        {
            for (id, entry) in stored where entry.count == 2 {
                roots[id] = entry[0]
                names[id] = String(decoding: entry[1], as: UTF8.self)
            }
        }
    }

    static var documentsFolder: URL {
        FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
    }

    func openDocument(uri: String) -> Int32 {
        guard let (url, scoped) = try? resolved(uri) else { return -1 }
        defer { if scoped { url.stopAccessingSecurityScopedResource() } }
        return Darwin.open(url.path, O_RDONLY | O_CLOEXEC)
    }

    func folders() -> [MusicFolder] {
        let (known, cached) = lock.withLock { (names, access) }
        let local = FolderAccess.of(Self.documentsFolder)
        let chosen = known.map { id, name in
            folder(id, name, cached[id] ?? recordAccess(id))
        }
        return ([folder(Self.local, "On this iPhone", local)] + chosen).sorted { first, second in
            if (first.id == Self.local) != (second.id == Self.local) { return first.id == Self.local }
            return first.name.lowercased() < second.name.lowercased()
        }
    }

    func refreshed() async -> [MusicFolder] {
        let known = lock.withLock { Array(names.keys) }
        try? await Work.aside { [self] in
            for id in known { recordAccess(id) }
        }
        return folders()
    }

    func add(_ url: URL) throws -> String {
        let scoped = url.startAccessingSecurityScopedResource()
        defer { if scoped { url.stopAccessingSecurityScopedResource() } }

        let chosen = FolderAccess.of(url)
        let known = lock.withLock { Array(names.keys) }
        let granted = known.map { id in lock.withLock { access[id] } ?? recordAccess(id) }.map(\.path)
        let documents = FolderAccess.of(Self.documentsFolder).path
        guard !(granted + [documents]).contains(chosen.path) else { throw FolderFailure.alreadyAdded(url.lastPathComponent) }

        let bookmark = try url.bookmarkData(options: [], includingResourceValuesForKeys: nil, relativeTo: nil)
        let id = UUID().uuidString.replacingOccurrences(of: "-", with: "")
        lock.withLock {
            roots[id] = bookmark
            names[id] = url.lastPathComponent
            access[id] = chosen
        }
        try persist()
        return id
    }

    func remove(_ id: String) throws {
        lock.withLock {
            scans[id]?.stop()
            roots[id] = nil
            names[id] = nil
            access[id] = nil
        }
        try persist()
    }

    func stopScanning(_ id: String) {
        lock.withLock { scans[id]?.stop() }
    }

    func uri(root: String, relative: String) -> String {
        let escaped = relative.split(separator: "/").map {
            String($0).addingPercentEncoding(withAllowedCharacters: .urlPathAllowed.subtracting(["/", "%", "?", "#"])) ?? String($0)
        }
        return "\(Self.scheme)://\(root)/\(escaped.joined(separator: "/"))"
    }

    func scan(_ id: String, found: (FoundDocument) throws -> Void) throws {
        let stop = ScanStop()
        lock.withLock { scans[id] = stop }
        defer { lock.withLock { if scans[id] === stop { scans[id] = nil } } }

        let (root, scoped) = try reachedRoot(id)
        defer { if scoped { root.stopAccessingSecurityScopedResource() } }

        var pending: [(URL, String, Int)] = [(root, "", 0)]
        while !stop.isStopped, let (folder, relative, depth) = pending.popLast() {
            let children = (try? FileManager.default.contentsOfDirectory(
                at: folder,
                includingPropertiesForKeys: [.isDirectoryKey],
                options: [.skipsHiddenFiles]
            )) ?? []
            var audio: [(URL, String)] = []
            var sidecars: [String: String] = [:]
            var covers: [String: String] = [:]
            for child in children {
                let name = child.lastPathComponent
                let path = relative.isEmpty ? name : "\(relative)/\(name)"
                let lower = name.lowercased()
                if (try? child.resourceValues(forKeys: [.isDirectoryKey]).isDirectory) == true {
                    if depth < Self.deepest { pending.append((child, path, depth + 1)) }
                } else if extensions.contains((lower as NSString).pathExtension) {
                    audio.append((child, path))
                } else if lower.hasSuffix(".lrc") {
                    sidecars[String(lower.dropLast(4))] = uri(root: id, relative: path)
                } else if Self.coverNames.contains(lower) {
                    covers[lower] = uri(root: id, relative: path)
                }
            }
            let cover = Self.coverNames.lazy.compactMap { covers[$0] }.first
            for (file, path) in audio.sorted(by: { $0.1 < $1.1 }) where !stop.isStopped {
                let name = file.lastPathComponent
                let stem = (name as NSString).deletingPathExtension.lowercased()
                try found(FoundDocument(uri: uri(root: id, relative: path), name: name, cover: cover, lyrics: sidecars[stem]))
            }
        }
    }

    func companion(_ uri: String, sidecar: Bool) -> Data? {
        guard let (url, scoped) = try? resolved(uri) else { return nil }
        defer { if scoped { url.stopAccessingSecurityScopedResource() } }
        let limit = sidecar ? Self.sidecarLimit : Self.coverLimit
        guard let handle = try? FileHandle(forReadingFrom: url),
            let data = try? handle.read(upToCount: limit + 1)
        else { return nil }
        try? handle.close()
        return data.count <= limit ? data : nil
    }

    func fresh(in id: String, named name: String) throws -> (uri: String, descriptor: Int32) {
        let (root, scoped) = try root(id)
        defer { if scoped { root.stopAccessingSecurityScopedResource() } }
        let stem = (name as NSString).deletingPathExtension
        let suffix = (name as NSString).pathExtension
        for attempt in 0..<1000 {
            let candidate = attempt == 0 ? name : "\(stem) (\(attempt)).\(suffix)"
            let path = root.appendingPathComponent(candidate).path
            let descriptor = Darwin.open(path, O_RDWR | O_CREAT | O_EXCL | O_CLOEXEC, 0o644)
            if descriptor >= 0 {
                return (uri(root: id, relative: candidate), descriptor)
            }
            if errno != EEXIST { break }
        }
        throw FolderFailure.refused(name)
    }

    func delete(_ uri: String) {
        guard let (url, scoped) = try? resolved(uri) else { return }
        defer { if scoped { url.stopAccessingSecurityScopedResource() } }
        try? FileManager.default.removeItem(at: url)
    }

    private func folder(_ id: String, _ name: String, _ found: FolderAccess) -> MusicFolder {
        MusicFolder(id: id, name: name, path: found.path, readable: found.readable, writable: found.readable && found.writable)
    }

    @discardableResult
    private func recordAccess(_ id: String) -> FolderAccess {
        guard let (url, scoped) = try? root(id) else {
            remember(.unreachable, for: id)
            return .unreachable
        }
        defer { if scoped { url.stopAccessingSecurityScopedResource() } }
        let found = FolderAccess.of(url)

        remember(found, for: id)
        return found
    }

    private func reachedRoot(_ id: String) throws -> (URL, Bool) {
        do {
            let (url, scoped) = try root(id)
            remember(FolderAccess.of(url), for: id)
            return (url, scoped)
        } catch {
            remember(.unreachable, for: id)
            throw error
        }
    }

    private func remember(_ found: FolderAccess, for id: String) {
        lock.withLock {
            if names[id] != nil { access[id] = found }
        }
    }

    private func root(_ id: String) throws -> (URL, Bool) {
        if id == Self.local { return (Self.documentsFolder, false) }
        guard let bookmark = lock.withLock({ roots[id] }) else { throw FolderFailure.unknownFolder(id) }
        var stale = false
        let url = try URL(resolvingBookmarkData: bookmark, options: [], relativeTo: nil, bookmarkDataIsStale: &stale)
        let scoped = url.startAccessingSecurityScopedResource()
        if stale, let renewed = try? url.bookmarkData(options: [], includingResourceValuesForKeys: nil, relativeTo: nil) {
            lock.withLock { roots[id] = renewed }
            try? persist()
        }
        return (url, scoped)
    }

    private func resolved(_ uri: String) throws -> (URL, Bool) {
        let prefix = "\(Self.scheme)://"
        guard uri.hasPrefix(prefix) else { throw FolderFailure.notAFolderUri(uri) }
        let rest = uri.dropFirst(prefix.count)
        let (id, relative) = rest.firstIndex(of: "/").map { (String(rest[..<$0]), String(rest[rest.index(after: $0)...])) }
            ?? (String(rest), "")
        let (root, scoped) = try root(id)
        let path = relative.split(separator: "/").map { $0.removingPercentEncoding ?? String($0) }
        guard !path.contains("..") else {
            if scoped { root.stopAccessingSecurityScopedResource() }
            throw FolderFailure.notAFolderUri(uri)
        }
        return (path.reduce(root) { $0.appendingPathComponent($1) }, scoped)
    }

    private func persist() throws {
        let stored = lock.withLock {
            roots.reduce(into: [String: [Data]]()) { kept, entry in
                kept[entry.key] = [entry.value, Data((names[entry.key] ?? "").utf8)]
            }
        }
        try FileManager.default.createDirectory(at: bookmarks.deletingLastPathComponent(), withIntermediateDirectories: true)
        try PropertyListEncoder().encode(stored).write(to: bookmarks, options: .atomic)
    }
}
