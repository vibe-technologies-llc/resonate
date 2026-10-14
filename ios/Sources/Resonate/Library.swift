import Accessibility
import Foundation
import Observation
import ResonateNative

typealias NativeAction = @Sendable (MobilePlayer) async -> Void

struct StatusMessage: Equatable {
    enum Kind {
        case notice
        case failure
    }

    let text: String
    let kind: Kind
    let serial: Int
}

@MainActor
@Observable
final class PlayClock {
    private(set) var positionMs: UInt64 = 0
    private(set) var durationMs: UInt64 = 0
    private(set) var sleepRemainingMs: UInt64?
    private(set) var outputRate: UInt32 = 0
    private(set) var underruns: UInt64 = 0

    func follow(_ state: Snapshot) {
        if positionMs != state.positionMs { positionMs = state.positionMs }
        if durationMs != state.durationMs { durationMs = state.durationMs }
        if sleepRemainingMs != state.sleepRemainingMs { sleepRemainingMs = state.sleepRemainingMs }
        if outputRate != state.outputRate { outputRate = state.outputRate }
        if underruns != state.underruns { underruns = state.underruns }
    }
}

struct Steered {
    var transport: Transport?
    var shuffle: Bool?
    var repeatMode: UInt32?

    func applied(to state: Snapshot) -> Snapshot {
        var steered = state
        if let transport { steered.transport = transport }
        if let shuffle { steered.shuffle = shuffle }
        if let repeatMode { steered.repeat = repeatMode }
        return steered
    }
}

struct Settling {
    private(set) var asked = 0
    private(set) var answered = 0

    var settled: Bool { asked == answered }

    mutating func ask() {
        asked += 1
    }

    mutating func answer() {
        answered += 1
    }
}

@MainActor
@Observable
final class Library {
    nonisolated static let page: UInt32 = 500
    private static let tick: Duration = .milliseconds(250)
    private static let ticksBetweenSaves = 20
    private static let noticeLasts: Duration = .seconds(6)

    let folders: Folders
    let clock = PlayClock()
    var allTracks: [Track] = [] {
        didSet { indexCatalog() }
    }
    var tracks: [Track] = []
    var queue: [Track] = []
    var snapshot: Snapshot?
    var status: StatusMessage?
    var scanning = false
    var scanned = 0
    var preferences: PlaybackPreferences?
    var filter: TrackFilter = defaultTrackFilter()
    var search = "" {
        didSet { reload() }
    }
    var folderList: [MusicFolder] = []
    private(set) var currentId: UInt64?
    private(set) var currentQueueId: UInt64?
    private(set) var favouriteIds: Set<UInt64> = []

    @ObservationIgnored private(set) var native: MobilePlayer?
    @ObservationIgnored private var waiting: [CheckedContinuation<MobilePlayer, Swift.Error>] = []
    @ObservationIgnored private var openingFailure: (any Swift.Error)?
    @ObservationIgnored private let actions: AsyncStream<NativeAction>
    @ObservationIgnored private let enqueue: AsyncStream<NativeAction>.Continuation
    @ObservationIgnored private var polling: Task<Void, Never>?
    @ObservationIgnored private var scanTask: Task<Void, Never>?
    @ObservationIgnored private var queuedScans: [String] = []
    @ObservationIgnored private var scanningFolder: String?
    @ObservationIgnored private var scanStopped = false
    @ObservationIgnored private var queryRequest = 0
    @ObservationIgnored private let preferencesStore = PlaybackPreferencesStore()
    @ObservationIgnored private var catalogIds: Set<UInt64> = []
    @ObservationIgnored private var shownFailure: String?
    @ObservationIgnored private var pollFailure: String?
    @ObservationIgnored private var statusSerial = 0
    @ObservationIgnored private var fading: Task<Void, Never>?
    @ObservationIgnored private var steering = Settling()
    @ObservationIgnored private var steered = Steered()
    @ObservationIgnored private var queueSettling = Settling()
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
        Task { [weak self] in
            let measured = await documents.refreshed()
            self?.folderList = measured
        }
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
            let steeringAsked = steering.asked
            let steeringSettled = steering.settled
            let queueAsked = queueSettling.asked
            let queueSettled = queueSettling.settled
            do {
                let (state, rows) = try await Work.inOrder {
                    let state = try native.snapshot()
                    let rows = try native.queue()
                    if saving { try native.save() }
                    return (state, rows)
                }
                let steeringFresh = steeringSettled && steering.asked == steeringAsked
                let queueFresh = queueSettled && queueSettling.asked == queueAsked

                pollFailure = nil
                publish(steeringFresh ? state : steered.applied(to: state))
                if queueFresh, queue != rows { queue = rows }
                nowPlaying.update(state)
                await features.tick(native, state)
            } catch {
                reportPolling(error)
            }
            try? await Task.sleep(for: Self.tick)
        }
    }

    private func publish(_ state: Snapshot) {
        clock.follow(state)
        if snapshot.map(Self.steady) != Self.steady(state) { snapshot = state }
        if currentId != state.current?.id {
            currentId = state.current?.id
            shownFailure = nil
        }
        if currentQueueId != state.current?.queueId { currentQueueId = state.current?.queueId }
        if let failure = state.failure, failure != shownFailure {
            shownFailure = failure
            show(failure, as: .failure)
        }
    }

    private static func steady(_ state: Snapshot) -> Snapshot {
        var steady = state
        steady.positionMs = 0
        steady.durationMs = 0
        steady.sleepRemainingMs = state.sleepRemainingMs.map { _ in 0 }
        steady.outputRate = 0
        steady.underruns = 0
        steady.failure = nil
        return steady
    }

    private func reportPolling(_ failure: any Swift.Error) {
        let text = explained(failure)
        guard text != pollFailure else { return }
        pollFailure = text
        show(text, as: .failure)
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

    private func attempt(
        _ action: @escaping @Sendable (MobilePlayer) throws -> Void,
        answered: @escaping @MainActor @Sendable ((any Swift.Error)?) -> Void
    ) {
        enqueue.yield { native in
            do {
                try await Work.inOrder { try action(native) }
                await MainActor.run { answered(nil) }
            } catch {
                await MainActor.run { answered(error) }
            }
        }
    }

    var transport: Transport {
        snapshot?.transport ?? .stopped
    }

    var loaded: Bool {
        snapshot?.current != nil
    }

    var playingQueueRow: Int? {
        guard let currentQueueId else { return nil }
        return queue.firstIndex { $0.queueId == currentQueueId }
    }

    func isFavourite(_ track: Track) -> Bool {
        catalogIds.contains(track.id) ? favouriteIds.contains(track.id) : track.favourite
    }

    func track(_ id: UInt64) -> Track? {
        if let current = snapshot?.current, current.id == id { return current }
        return allTracks.first { $0.id == id }
            ?? queue.first { $0.id == id }
            ?? features.playlistRows.first { $0.id == id }
            ?? features.statistics?.topTracks.first { $0.id == id }
    }

    private func indexCatalog() {
        catalogIds = Set(allTracks.map(\.id))
        favouriteIds = Set(allTracks.lazy.filter(\.favourite).map(\.id))
    }

    func play(_ rows: [Track], at index: Int, shuffle: Bool = false) {
        guard !rows.isEmpty else { return }
        let ids = rows.map(\.id)
        let start = UInt32(min(max(index, 0), rows.count - 1))

        nowPlaying.activate()
        steer(.loading) { native in
            try native.load(ids: ids, startAt: start, autoplay: false)
            try native.setShuffle(shuffled: shuffle)
            try native.play()
        }
    }

    func play(_ rows: [Track], selected: Track) {
        play(rows, at: rows.firstIndex { $0.id == selected.id } ?? 0)
    }

    func shuffle(_ rows: [Track]) {
        play(rows, at: Int.random(in: 0..<max(rows.count, 1)), shuffle: true)
    }

    func jump(to row: Track) {
        guard let index = queue.firstIndex(where: { $0.queueId == row.queueId }) else { return }
        nowPlaying.activate()
        steer(.loading) { native in
            try native.jumpTo(index: UInt32(index))
            try native.play()
        }
    }

    func toggle() {
        switch transport {
        case .playing, .loading:
            steer(.paused) { try $0.pause() }
        case .paused, .stopped:
            nowPlaying.activate()
            steer(.playing) { try $0.play() }
        }
    }

    func resume() {
        guard transport != .playing, transport != .loading else { return }
        nowPlaying.activate()
        steer(.playing) { try $0.play() }
    }

    func pause() {
        guard transport == .playing || transport == .loading else { return }
        steer(.paused) { try $0.pause() }
    }

    func stop() {
        steer(.stopped) { try $0.stop() }
    }

    func next() {
        request { try $0.next() }
    }

    func previous() {
        request { try $0.previous() }
    }

    func seek(to position: UInt64) {
        request { try $0.seek(positionMs: position) }
    }

    func setShuffle(_ shuffled: Bool) {
        steer({ $0.shuffle = shuffled }) { try $0.setShuffle(shuffled: shuffled) }
    }

    func setRepeat(_ mode: RepeatMode) {
        let value = mode.engineValue
        steer({ $0.repeatMode = value }) { try $0.setRepeat(repeat: value) }
    }

    var repeatMode: RepeatMode {
        RepeatMode(engineValue: snapshot?.repeat ?? 0)
    }

    private func steer(_ expected: Transport, _ action: @escaping @Sendable (MobilePlayer) throws -> Void) {
        steer({ $0.transport = expected }, action)
    }

    private func steer(_ change: (inout Steered) -> Void, _ action: @escaping @Sendable (MobilePlayer) throws -> Void) {
        steering.ask()
        change(&steered)
        if let shown = snapshot {
            let expected = steered.applied(to: shown)
            if expected != shown { snapshot = expected }
        }
        attempt(action) { [self] failure in
            steering.answer()
            if steering.settled { steered = Steered() }
            if let failure { report(failure) }
        }
    }

    func playNext(_ rows: [Track]) {
        let ids = rows.map(\.id)
        guard !ids.isEmpty else { return }
        editQueue(rows.count == 1 ? "Playing next" : "Playing \(counted(rows.count, "song")) next") {
            try $0.playNext(ids: ids)
        }
    }

    func addToQueue(_ rows: [Track]) {
        let ids = rows.map(\.id)
        guard !ids.isEmpty else { return }
        editQueue(rows.count == 1 ? "Added to queue" : "Added \(counted(rows.count, "song")) to queue") {
            try $0.addToQueue(ids: ids)
        }
    }

    func removeFromQueue(_ row: Track) {
        guard let queueId = row.queueId else { return }
        queue.removeAll { $0.queueId == queueId }
        editQueue { try $0.removeFromQueue(queueId: queueId) }
    }

    func moveInQueue(from: Int, to: Int) {
        guard queue.indices.contains(from), (0...queue.count).contains(to), to != from, to != from + 1 else { return }
        queue.move(fromOffsets: IndexSet(integer: from), toOffset: to)
        editQueue { try $0.moveInQueue(from: UInt32(from), to: UInt32(to)) }
    }

    func clearQueue() {
        let playing = currentQueueId
        queue = queue.filter { playing != nil && $0.queueId == playing }
        editQueue { try $0.clearQueue() }
    }

    func rereadQueue() async {
        guard let native = try? await player() else { return }
        do {
            let rows = try await Work.inOrder { try native.queue() }
            if queueSettling.settled { queue = rows }
        } catch {
            report(error)
        }
    }

    private func editQueue(_ done: String? = nil, _ action: @escaping @Sendable (MobilePlayer) throws -> Void) {
        queueSettling.ask()
        attempt(action) { [self] failure in
            queueSettling.answer()
            if let failure {
                report(failure)
            } else if let done {
                notice(done)
            }
            Task { await rereadQueue() }
        }
    }

    func setFavourite(_ track: Track, _ favourite: Bool) {
        let id = track.id

        markFavourite(id, favourite)
        attempt({ native in try native.favourite(id: id, favourite: favourite) }) { [self] failure in
            guard let failure else { return }
            markFavourite(id, !favourite)
            show(
                (favourite ? "Could not add this song to favourites: " : "Could not remove this song from favourites: ")
                    + explained(failure),
                as: .failure
            )
        }
    }

    private func markFavourite(_ id: UInt64, _ favourite: Bool) {
        func marked(_ rows: [Track]) -> [Track] {
            guard rows.contains(where: { $0.id == id && $0.favourite != favourite }) else { return rows }
            return rows.map { row in
                var row = row
                if row.id == id { row.favourite = favourite }
                return row
            }
        }

        allTracks = marked(allTracks)
        tracks = marked(tracks).filter { !filter.favourites || $0.favourite }
        queue = marked(queue)
        if snapshot?.current?.id == id { snapshot?.current?.favourite = favourite }
        features.playlistRows = marked(features.playlistRows)
        if let top = features.statistics?.topTracks { features.statistics?.topTracks = marked(top) }
        if features.inspector?.track.id == id { features.inspector?.track.favourite = favourite }
        nowPlaying.favouriteChanged()
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
        queuedScans.removeAll { $0 == folder.id }
        if scanningFolder == folder.id { scanStopped = true }
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

    func stopScanning() {
        queuedScans.removeAll()
        guard let scanningFolder else { return }
        scanStopped = true
        folders.stopScanning(scanningFolder)
    }

    func scan(_ id: String) {
        guard !queuedScans.contains(id), scanningFolder != id else { return }
        queuedScans.append(id)
        let previous = scanTask
        scanTask = Task { [weak self] in
            await previous?.value
            await self?.walk(id)
        }
    }

    private func walk(_ id: String) async {
        guard let queued = queuedScans.firstIndex(of: id) else { return }
        queuedScans.remove(at: queued)
        guard let native = try? await player() else { return }
        let folders = folders
        let tally = Tally()

        scanning = true
        scanned = 0
        scanStopped = false
        scanningFolder = id
        defer {
            scanningFolder = nil
            scanning = false
        }
        var failure: (any Swift.Error)?
        do {
            try await Work.aside { [weak self] in
                try folders.scan(id) { document in
                    do {
                        let track = try native.indexDocument(uri: document.uri, name: document.name)
                        let read = tally.indexed()
                        Task { @MainActor in self?.reached(read) }
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
            failure = error
        }
        await refresh()
        features.refreshArtwork()
        folderList = folders.folders()
        if let failure {
            report(failure)
        } else {
            notice(scanStopped ? tally.stoppedSummary : tally.summary)
        }
    }

    private func reached(_ read: Int) {
        if scanning, read > scanned { scanned = read }
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
        show(message, as: .notice)
    }

    func report(_ failure: any Swift.Error) {
        show(explained(failure), as: .failure)
    }

    func dismissStatus() {
        fading?.cancel()
        status = nil
    }

    private func show(_ text: String, as kind: StatusMessage.Kind) {
        statusSerial += 1
        let shown = StatusMessage(text: text, kind: kind, serial: statusSerial)

        status = shown
        AccessibilityNotification.Announcement(text).post()
        fading?.cancel()
        guard kind == .notice else { return }
        fading = Task { [weak self] in
            try? await Task.sleep(for: Self.noticeLasts)
            guard !Task.isCancelled, let self, status?.serial == shown.serial else { return }
            status = nil
        }
    }

    func save() {
        request { try $0.save() }
    }
}

enum RepeatMode: CaseIterable {
    case off
    case all
    case one

    init(engineValue: UInt32) {
        switch engineValue {
        case 1: self = .one
        case 2: self = .all
        default: self = .off
        }
    }

    var engineValue: UInt32 {
        switch self {
        case .off: 0
        case .one: 1
        case .all: 2
        }
    }

    var following: RepeatMode {
        switch self {
        case .off: .all
        case .all: .one
        case .one: .off
        }
    }

    var symbol: String {
        self == .one ? "repeat.1" : "repeat"
    }

    var spoken: String {
        switch self {
        case .off: "Off"
        case .all: "All songs"
        case .one: "This song"
        }
    }
}

final class Tally: @unchecked Sendable {
    private let lock = NSLock()
    private var read = 0
    private var unreadable = 0

    func indexed() -> Int {
        lock.withLock {
            read += 1
            return read
        }
    }

    func failed() { lock.withLock { unreadable += 1 } }

    var summary: String {
        lock.withLock {
            "Read \(counted(read, "song"))" + (unreadable > 0 ? "; \(unreadable) unreadable" : "")
        }
    }

    var stoppedSummary: String {
        lock.withLock {
            "Stopped reading after \(counted(read, "song"))" + (unreadable > 0 ? "; \(unreadable) unreadable" : "")
        }
    }
}
