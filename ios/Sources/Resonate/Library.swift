import Foundation
import Observation
import ResonateNative

typealias NativeAction = @Sendable (MobilePlayer) async -> Void

@MainActor
@Observable
final class Library {
    nonisolated static let page: UInt32 = 500
    private static let tick: Duration = .milliseconds(250)
    private static let ticksBetweenSaves = 20

    let folders: Folders
    var allTracks: [Track] = []
    var tracks: [Track] = []
    var queue: [Track] = []
    var snapshot: Snapshot?
    var status: String?
    var scanning = false
    var preferences: PlaybackPreferences?
    var filter: TrackFilter = defaultTrackFilter()
    var search = "" {
        didSet { reload() }
    }
    var folderList: [MusicFolder] = []

    @ObservationIgnored private(set) var native: MobilePlayer?
    @ObservationIgnored private var waiting: [CheckedContinuation<MobilePlayer, Swift.Error>] = []
    @ObservationIgnored private var openingFailure: (any Swift.Error)?
    @ObservationIgnored private let actions: AsyncStream<NativeAction>
    @ObservationIgnored private let enqueue: AsyncStream<NativeAction>.Continuation
    @ObservationIgnored private var polling: Task<Void, Never>?
    @ObservationIgnored private var scanTask: Task<Void, Never>?
    @ObservationIgnored private var queryRequest = 0
    @ObservationIgnored private let preferencesStore = PlaybackPreferencesStore()
    @ObservationIgnored let support: URL
    @ObservationIgnored lazy var features = Features(library: self)
    @ObservationIgnored lazy var nowPlaying = NowPlaying(library: self)

    init() {
        support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("Resonate", isDirectory: true)
        folders = Folders(support: support)
        (actions, enqueue) = AsyncStream.makeStream(of: NativeAction.self)
        Task { [weak self] in await self?.serveActions() }
    }

    func start() {
        guard polling == nil else { return }
        let folder = support.path
        let documents = folders
        folderList = folders.folders()
        polling = Task { [weak self] in
            do {
                let opened = try await Work.inOrder {
                    try MobilePlayer(folder: folder, documents: documents)
                }
                guard let self else { return }
                let stored = preferencesStore.read()
                try await Work.inOrder { try opened.configurePlayback(preferences: stored) }
                preferences = stored
                native = opened
                waiting.forEach { $0.resume(returning: opened) }
                waiting.removeAll()
                nowPlaying.start(opened)
                features.initialize(opened)
                await refresh()
                await poll(opened)
            } catch {
                guard let self else { return }
                openingFailure = error
                waiting.forEach { $0.resume(throwing: error) }
                waiting.removeAll()
                report(error)
            }
        }
    }

    func player() async throws -> MobilePlayer {
        if let native { return native }
        if let openingFailure { throw openingFailure }
        return try await withCheckedThrowingContinuation { waiting.append($0) }
    }

    private func serveActions() async {
        for await action in actions {
            guard let native = try? await player() else { continue }
            await action(native)
        }
    }

    private func poll(_ native: MobilePlayer) async {
        var ticks = 0
        while !Task.isCancelled {
            ticks += 1
            let saving = ticks % Self.ticksBetweenSaves == 0
            do {
                let (state, rows) = try await Work.inOrder {
                    let state = try native.snapshot()
                    let rows = try native.queue()
                    if saving { try native.save() }
                    return (state, rows)
                }
                snapshot = state
                if queue != rows { queue = rows }
                if let failure = state.failure { status = failure }
                nowPlaying.update(state)
                await features.tick(native, state)
            } catch {
                report(error)
            }
            try? await Task.sleep(for: Self.tick)
        }
    }

    func request(_ action: @escaping @Sendable (MobilePlayer) throws -> Void) {
        enqueue.yield { native in
            do {
                try await Work.inOrder { try action(native) }
            } catch {
                await MainActor.run { self.report(error) }
            }
        }
    }

    func request<Value: Sendable>(
        _ action: @escaping @Sendable (MobilePlayer) throws -> Value,
        then: @escaping @MainActor @Sendable (Value) -> Void
    ) {
        enqueue.yield { native in
            do {
                let value = try await Work.inOrder { try action(native) }
                await MainActor.run { then(value) }
            } catch {
                await MainActor.run { self.report(error) }
            }
        }
    }

    func play(_ rows: [Track], at index: Int, shuffle: Bool = false) {
        guard !rows.isEmpty else { return }
        let ids = rows.map(\.id)
        let start = UInt32(min(max(index, 0), rows.count - 1))
        nowPlaying.activate()
        request { native in
            try native.load(ids: ids, startAt: start, autoplay: false)
            try native.setShuffle(shuffled: shuffle)
            try native.play()
        }
    }

    func play(_ rows: [Track], selected: Track) {
        play(rows, at: rows.firstIndex { $0.id == selected.id } ?? 0)
    }

    func jump(to row: Track) {
        guard let index = queue.firstIndex(where: { $0.queueId == row.queueId }) else { return }
        nowPlaying.activate()
        request { native in
            try native.jumpTo(index: UInt32(index))
            try native.play()
        }
    }

    func toggle() {
        if snapshot?.transport == .playing {
            request { try $0.pause() }
        } else {
            nowPlaying.activate()
            request { try $0.play() }
        }
    }

    func setFavourite(_ track: Track, _ favourite: Bool) {
        let id = track.id
        request({ native in try native.favourite(id: id, favourite: favourite) }) { [self] in
            func updated(_ rows: [Track]) -> [Track] {
                rows.map { row in
                    var row = row
                    if row.id == id { row.favourite = favourite }
                    return row
                }
            }
            allTracks = updated(allTracks)
            tracks = updated(tracks).filter { !filter.favourites || $0.favourite }
            queue = updated(queue)
            if var state = snapshot, state.current?.id == id {
                state.current?.favourite = favourite
                snapshot = state
            }
            features.playlistRows = updated(features.playlistRows)
            reload()
        }
    }

    func updatePreferences(_ change: @escaping @Sendable (PlaybackPreferences) -> PlaybackPreferences) {
        let store = preferencesStore
        request({ native in
            let next = change(native.playbackPreferences())
            try native.configurePlayback(preferences: next)
            store.write(next)
            return next
        }) { [self] next in preferences = next }
    }

    func setFilter(_ value: TrackFilter) {
        filter = value
        reload()
    }

    func refresh() async {
        guard let native = try? await player() else { return }
        do {
            allTracks = try await Work.inOrder {
                var found: [Track] = []
                while true {
                    let page = try native.filteredTracks(
                        filter: defaultTrackFilter(),
                        offset: UInt32(found.count),
                        limit: Library.page
                    )
                    found += page
                    if page.count < Library.page { return found }
                }
            }
        } catch {
            report(error)
        }
        reload()
    }

    func reload() {
        queryRequest += 1
        let request = queryRequest
        let query = {
            var query = filter
            query.text = search
            return query
        }()
        Task {
            guard let native = try? await player() else { return }
            do {
                let found = try await Work.inOrder {
                    var found: [Track] = []
                    while true {
                        let page = try native.filteredTracks(filter: query, offset: UInt32(found.count), limit: Library.page)
                        found += page
                        if page.count < Library.page { return found }
                    }
                }
                if request == queryRequest { tracks = found }
            } catch {
                report(error)
            }
        }
    }

    func addFolder(_ url: URL) {
        do {
            let id = try folders.add(url)
            folderList = folders.folders()
            scan(id)
        } catch {
            report(error)
        }
    }

    func removeFolder(_ folder: MusicFolder) {
        do {
            try folders.remove(folder.id)
            folderList = folders.folders()
        } catch {
            report(error)
        }
    }

    func rescan() {
        folderList = folders.folders()
        for folder in folderList where folder.readable {
            scan(folder.id)
        }
    }

    func scan(_ id: String) {
        let previous = scanTask
        let folders = folders
        scanTask = Task { [weak self] in
            await previous?.value
            guard let self, let native = try? await player() else { return }
            scanning = true
            status = "Reading music folder…"
            defer { scanning = false }
            let tally = Tally()
            do {
                try await Work.aside {
                    try folders.scan(id) { document in
                        do {
                            let track = try native.indexDocument(uri: document.uri, name: document.name)
                            tally.indexed()
                            if let cover = document.cover, let bytes = folders.companion(cover, sidecar: false) {
                                try? native.setArtwork(id: track.id, bytes: bytes)
                            }
                            if let lyrics = document.lyrics, let bytes = folders.companion(lyrics, sidecar: true) {
                                try? native.keepSidecarLyrics(id: track.id, bytes: bytes)
                            }
                        } catch {
                            tally.failed()
                        }
                    }
                }
            } catch {
                report(error)
            }
            await refresh()
            features.refreshArtwork()
            folderList = folders.folders()
            status = tally.summary
        }
    }

    func open(_ url: URL) {
        let folders = folders
        Task {
            do {
                let kept = try await Work.aside { try Library.kept(url, in: folders) }
                request({ native in try native.indexDocument(uri: kept.uri, name: kept.name) }) { [self] track in
                    Task {
                        await refresh()
                        play([track], at: 0)
                    }
                }
            } catch {
                report(error)
            }
        }
    }

    nonisolated private static func kept(_ url: URL, in folders: Folders) throws -> (uri: String, name: String) {
        let scoped = url.startAccessingSecurityScopedResource()
        defer { if scoped { url.stopAccessingSecurityScopedResource() } }
        let documents = Folders.documentsFolder.standardizedFileURL.path
        let path = url.standardizedFileURL.path
        if path.hasPrefix(documents + "/") {
            return (folders.uri(root: Folders.local, relative: String(path.dropFirst(documents.count + 1))), url.lastPathComponent)
        }
        let (uri, descriptor) = try folders.fresh(in: Folders.local, named: url.lastPathComponent)
        do {
            let destination = FileHandle(fileDescriptor: descriptor, closeOnDealloc: true)
            let source = try FileHandle(forReadingFrom: url)
            while let chunk = try source.read(upToCount: 1 << 20), !chunk.isEmpty {
                try destination.write(contentsOf: chunk)
            }
        } catch {
            folders.delete(uri)
            throw error
        }
        let name = (uri as NSString).lastPathComponent.removingPercentEncoding ?? url.lastPathComponent
        return (uri, name)
    }

    func notice(_ message: String) {
        status = message
    }

    func report(_ failure: any Swift.Error) {
        status = explained(failure)
    }

    func save() {
        request { try $0.save() }
    }
}

final class Tally: @unchecked Sendable {
    private let lock = NSLock()
    private var read = 0
    private var unreadable = 0

    func indexed() { lock.withLock { read += 1 } }
    func failed() { lock.withLock { unreadable += 1 } }

    var summary: String {
        lock.withLock {
            "Read \(counted(read, "song"))" + (unreadable > 0 ? "; \(unreadable) unreadable" : "")
        }
    }
}
