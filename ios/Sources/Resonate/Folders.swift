import Darwin
import Foundation
import ResonateLogic
import ResonateNative
import Synchronization

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
    let size: UInt64
    let modifiedMs: Int64
}

struct FileStamp: Sendable {
    let size: UInt64
    let modifiedMs: Int64

    init(_ values: URLResourceValues?) {
        size = values?.fileSize.map { UInt64(max($0, 0)) } ?? 0
        modifiedMs = values?.contentModificationDate.map { Int64(($0.timeIntervalSince1970 * 1000).rounded()) } ?? 0
    }
}

final class DocumentsWatch {
    private let source: any DispatchSourceFileSystemObject

    init?(changed: @escaping @Sendable () -> Void) {
        let descriptor = Darwin.open(Folders.documentsFolder.path, O_EVTONLY | O_CLOEXEC)
        guard descriptor >= 0 else { return nil }
        source = DispatchSource.makeFileSystemObjectSource(
            fileDescriptor: descriptor,
            eventMask: .write,
            queue: .global(qos: .utility)
        )
        source.setEventHandler(handler: changed)
        source.setCancelHandler { Darwin.close(descriptor) }
        source.resume()
    }

    deinit {
        source.cancel()
    }
}

enum FolderFailure: Swift.Error, LocalizedError {
    case unknownFolder(String)
    case notAFolderUri(String)
    case refused(String)
    case alreadyAdded(String)
    case overlapping(String)
    case unreadable(String)

    var errorDescription: String? {
        switch self {
        case .unknownFolder: "That music folder is no longer available"
        case .notAFolderUri: "That song is not inside a chosen music folder"
        case .refused(let name): "\(name) could not be created"
        case .alreadyAdded: "This folder is already in your library"
        case .overlapping(let name): "\(name) holds or sits inside a folder already in your library"
        case .unreadable(let name): "\(name) could not be read"
        }
    }
}

final class Folders: Documents, @unchecked Sendable {
    static let scheme = FolderURI.scheme
    static let local = "local"
    static let kept = ["folders.plist", "forgotten-folders.plist"]
    private static let deepest = 32
    private static let copiesChecked = 50
    private static let coverNames = ["cover.jpg", "cover.png", "folder.jpg", "folder.png", "front.jpg", "front.png"]
    private static let sidecarExtension = "lrc"
    private static let sidecarLimit = 1024 * 1024
    private static let coverLimit = 8 * 1024 * 1024

    private let lock = NSLock()
    private let bookmarks: URL
    private let forgottenFile: URL
    private var roots: [String: Data] = [:]
    private var resolvedRoots: [String: URL] = [:]
    private var names: [String: String] = [:]
    private var access: [String: FolderAccess] = [:]
    private var forgotten: [String: String] = [:]
    private var scans: [String: ScanStop] = [:]
    private var writing: Set<String> = []
    private let extensions: Set<String>

    init(support: URL) {
        bookmarks = support.appendingPathComponent("folders.plist")
        forgottenFile = support.appendingPathComponent("forgotten-folders.plist")
        extensions = Set(audioExtensions())
        if let data = try? Data(contentsOf: bookmarks),
            let stored = try? PropertyListDecoder().decode([String: [Data]].self, from: data)
        {
            for (id, entry) in stored where entry.count == 2 {
                roots[id] = entry[0]
                names[id] = String(decoding: entry[1], as: UTF8.self)
            }
        }
        if let data = try? Data(contentsOf: forgottenFile),
            let stored = try? PropertyListDecoder().decode([String: String].self, from: data)
        {
            forgotten = stored
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
            for id in known { recordAccess(id, fresh: true) }
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
        let held = (granted + [documents]).filter { !$0.isEmpty }
        guard !held.contains(chosen.path) else { throw FolderFailure.alreadyAdded(url.lastPathComponent) }
        guard !held.contains(where: { Self.overlap($0, chosen.path) }) else { throw FolderFailure.overlapping(url.lastPathComponent) }

        let bookmark = try url.bookmarkData(options: [], includingResourceValuesForKeys: nil, relativeTo: nil)
        let id = lock.withLock { forgotten[chosen.path] }
            ?? UUID().uuidString.replacingOccurrences(of: "-", with: "")
        lock.withLock {
            roots[id] = bookmark
            names[id] = url.lastPathComponent
            access[id] = chosen
            forgotten[chosen.path] = nil
        }
        try persist()
        return id
    }

    private static func overlap(_ first: String, _ second: String) -> Bool {
        first.hasPrefix(second + "/") || second.hasPrefix(first + "/")
    }

    func remove(_ id: String) throws {
        lock.withLock {
            scans[id]?.stop()
            if let path = access[id]?.path, !path.isEmpty { forgotten[path] = id }
            roots[id] = nil
            resolvedRoots[id] = nil
            names[id] = nil
            access[id] = nil
        }
        try persist()
    }

    func stopScanning(_ id: String) {
        lock.withLock { scans[id]?.stop() }
    }

    func uri(root: String, relative: String) -> String {
        FolderURI.make(root: root, relative: relative)
    }

    func scan(
        _ id: String,
        skipped: (String, any Swift.Error) -> Void,
        found: (FoundDocument) throws -> Void
    ) throws {
        let stop = ScanStop()
        lock.withLock { scans[id] = stop }
        defer { lock.withLock { if scans[id] === stop { scans[id] = nil } } }

        let (root, scoped) = try reachedRoot(id)
        defer { if scoped { root.stopAccessingSecurityScopedResource() } }

        var pending: [(URL, String, Int)] = [(root, "", 0)]
        while !stop.isStopped, let (folder, relative, depth) = pending.popLast() {
            let children: [URL]
            do {
                children = try FileManager.default.contentsOfDirectory(
                    at: folder,
                    includingPropertiesForKeys: [.isDirectoryKey, .fileSizeKey, .contentModificationDateKey],
                    options: [.skipsHiddenFiles]
                )
            } catch {
                if relative.isEmpty { throw FolderFailure.unreadable(lock.withLock { names[id] } ?? folder.lastPathComponent) }
                skipped(relative, error)
                continue
            }
            var audio: [(URL, String, FileStamp)] = []
            var sidecars: [String: String] = [:]
            var covers: [String: String] = [:]
            for child in children {
                let name = child.lastPathComponent
                let path = relative.isEmpty ? name : "\(relative)/\(name)"
                let lower = name.lowercased()
                let values = try? child.resourceValues(forKeys: [.isDirectoryKey, .fileSizeKey, .contentModificationDateKey])
                if values?.isDirectory == true {
                    if depth < Self.deepest { pending.append((child, path, depth + 1)) }
                } else if extensions.contains((lower as NSString).pathExtension) {
                    if !isBeingWritten(uri(root: id, relative: path)) { audio.append((child, path, FileStamp(values))) }
                } else if lower.hasSuffix(".lrc") {
                    sidecars[String(lower.dropLast(4))] = uri(root: id, relative: path)
                } else if Self.coverNames.contains(lower) {
                    covers[lower] = uri(root: id, relative: path)
                }
            }
            let cover = Self.coverNames.lazy.compactMap { covers[$0] }.first
            for (file, path, stamp) in audio.sorted(by: { $0.1 < $1.1 }) where !stop.isStopped {
                let name = file.lastPathComponent
                let stem = (name as NSString).deletingPathExtension.lowercased()
                try found(
                    FoundDocument(
                        uri: uri(root: id, relative: path),
                        name: name,
                        cover: cover,
                        lyrics: sidecars[stem],
                        size: stamp.size,
                        modifiedMs: stamp.modifiedMs
                    )
                )
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

    func granted(_ url: URL) -> String? {
        let path = url.resolvingSymlinksInPath().standardizedFileURL.path
        let known = lock.withLock { Array(names.keys) }
        let chosen = known.map { id in (id, lock.withLock { access[id] } ?? recordAccess(id)) }
        let roots = chosen + [(Self.local, FolderAccess.of(Self.documentsFolder))]
        let holding = roots
            .filter { $0.1.readable && !$0.1.path.isEmpty && path.hasPrefix($0.1.path + "/") }
            .max { $0.1.path.count < $1.1.path.count }
        guard let (id, access) = holding else { return nil }
        return uri(root: id, relative: String(path.dropFirst(access.path.count + 1)))
    }

    func localCopy(identicalTo url: URL) -> (uri: String, name: String)? {
        let files = FileManager.default
        let name = url.lastPathComponent
        let stem = (name as NSString).deletingPathExtension
        let suffix = (name as NSString).pathExtension
        for attempt in 0..<Self.copiesChecked {
            let candidate = attempt == 0 ? name : "\(stem) (\(attempt)).\(suffix)"
            let path = Self.documentsFolder.appendingPathComponent(candidate).path
            guard files.fileExists(atPath: path) else { return nil }
            if files.contentsEqual(atPath: path, andPath: url.path) {
                return (uri(root: Self.local, relative: candidate), candidate)
            }
        }
        return nil
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
                let created = uri(root: id, relative: candidate)
                lock.withLock { _ = writing.insert(created) }
                return (created, descriptor)
            }
            if errno != EEXIST { break }
        }
        throw FolderFailure.refused(name)
    }

    func written(_ uri: String) {
        lock.withLock { _ = writing.remove(uri) }
    }

    private func isBeingWritten(_ uri: String) -> Bool {
        lock.withLock { writing.contains(uri) }
    }

    func withPaths<Value>(_ uris: [String], _ body: ([SongFile]) throws -> Value) throws -> Value {
        var opened: [(URL, Bool)] = []
        defer { for (url, scoped) in opened where scoped { url.stopAccessingSecurityScopedResource() } }
        let files = try uris.map { uri in
            let (url, scoped) = try resolved(uri)
            opened.append((url, scoped))
            return SongFile(uri: uri, path: url.path)
        }
        return try body(files)
    }

    func localFile(_ uri: String) -> URL? {
        guard Self.root(of: uri) == Self.local, let (url, _) = try? resolved(uri) else { return nil }
        return url
    }

    static func root(of uri: String) -> String? {
        FolderURI.root(of: uri)
    }

    func prefix(of id: String) -> String {
        "\(Self.scheme)://\(id)"
    }

    func file(_ id: String, _ moves: [FiledSong]) throws -> FiledMoves {
        let (root, scoped) = try root(id)
        defer { if scoped { root.stopAccessingSecurityScopedResource() } }
        return FolderFiling.file(moves, in: id, at: root) { [self] uri in
            let (url, scoped) = try resolved(uri)
            return ResolvedDocument(url: url) { if scoped { url.stopAccessingSecurityScopedResource() } }
        }
    }

    @discardableResult
    func delete(_ uri: String) -> Bool {
        let gone = removed(uri, unreachableIsGone: true)
        if gone { written(uri) }
        return gone
    }

    func deleteSong(_ uri: String) -> Bool {
        let gone = removed(uri, unreachableIsGone: false)
        if gone { written(uri) }
        return gone
    }

    private func removed(_ uri: String, unreachableIsGone: Bool) -> Bool {
        let resolution: (URL, Bool)
        do {
            resolution = try resolved(uri)
        } catch is FolderFailure {
            return unreachableIsGone
        } catch {
            return false
        }
        let (url, scoped) = resolution
        defer { if scoped { url.stopAccessingSecurityScopedResource() } }
        do {
            try FileManager.default.removeItem(at: url)
            return true
        } catch {
            return !FileManager.default.fileExists(atPath: url.path)
        }
    }

    private func folder(_ id: String, _ name: String, _ found: FolderAccess) -> MusicFolder {
        MusicFolder(id: id, name: name, path: found.path, readable: found.readable, writable: found.readable && found.writable)
    }

    @discardableResult
    private func recordAccess(_ id: String, fresh: Bool = false) -> FolderAccess {
        guard let (url, scoped) = try? root(id, fresh: fresh) else {
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
            let (url, scoped) = try root(id, fresh: true)
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

    private func root(_ id: String, fresh: Bool = false) throws -> (URL, Bool) {
        if id == Self.local { return (Self.documentsFolder, false) }
        let (bookmark, kept) = lock.withLock { (roots[id], resolvedRoots[id]) }
        guard let bookmark else { throw FolderFailure.unknownFolder(id) }
        if !fresh, let kept { return (kept, kept.startAccessingSecurityScopedResource()) }
        var stale = false
        let url: URL
        do {
            url = try URL(resolvingBookmarkData: bookmark, options: [], relativeTo: nil, bookmarkDataIsStale: &stale)
        } catch {
            lock.withLock { resolvedRoots[id] = nil }
            throw error
        }
        let scoped = url.startAccessingSecurityScopedResource()
        lock.withLock { if roots[id] != nil { resolvedRoots[id] = url } }
        if stale, let renewed = try? url.bookmarkData(options: [], includingResourceValuesForKeys: nil, relativeTo: nil) {
            lock.withLock { roots[id] = renewed }
            try? persist()
        }
        return (url, scoped)
    }

    private func resolved(_ uri: String) throws -> (URL, Bool) {
        guard let (id, relative) = FolderURI.split(uri) else { throw FolderFailure.notAFolderUri(uri) }
        let (root, scoped) = try root(id)
        guard let path = FolderURI.components(of: relative) else {
            if scoped { root.stopAccessingSecurityScopedResource() }
            throw FolderFailure.notAFolderUri(uri)
        }
        return (path.reduce(root) { $0.appendingPathComponent($1) }, scoped)
    }

    private func persist() throws {
        let (stored, remembered) = lock.withLock {
            let stored = roots.reduce(into: [String: [Data]]()) { kept, entry in
                kept[entry.key] = [entry.value, Data((names[entry.key] ?? "").utf8)]
            }
            return (stored, forgotten)
        }
        try FileManager.default.createDirectory(at: bookmarks.deletingLastPathComponent(), withIntermediateDirectories: true)
        try PropertyListEncoder().encode(stored).write(to: bookmarks, options: .atomic)
        try PropertyListEncoder().encode(remembered).write(to: forgottenFile, options: .atomic)
    }
}
