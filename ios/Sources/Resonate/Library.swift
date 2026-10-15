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

struct FavouriteWrite {
    let value: Bool
    let serial: Int
    var answeredAfter: Int?
}

enum PollPart: Hashable {
    case snapshot
    case queue
    case save
}

struct QueueListed: Equatable {
    let queue: UInt64
    let catalog: UInt64

    init(_ state: Snapshot) {
        queue = state.queueRevision
        catalog = state.catalogRevision
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
    var tracks: [Track] = [] {
        didSet { trackRuns = filter.order == .title ? LetterRun.runs(of: tracks, named: \.shownTitle) : [] }
    }
    private(set) var trackRuns: [LetterRun] = []
    private(set) var tracksRead = false
    private(set) var catalogRead = false
    private(set) var artworkTracks: [UInt64: Track] = [:]
    private(set) var albums: [AlbumHit] = [] {
        didSet { albumRuns = Self.runs(of: albums, in: collectionOrder) }
    }
    private(set) var albumRuns: [LetterRun] = []
    private(set) var artists: [ArtistHit] = [] {
        didSet { artistRuns = Self.runs(of: artists, in: collectionOrder) }
    }
    private(set) var artistRuns: [LetterRun] = []
    private(set) var collectionGeneration = 0
    var collectionOrder = CollectionOrder.alphabetical {
        didSet { if collectionOrder != oldValue { reloadCollections() } }
    }
    var queue: [Track] = []
    var snapshot: Snapshot?
    var status: StatusMessage?
    var scanning = false
    private(set) var erasing = false
    var scanned = 0
    var preferences: PlaybackPreferences?
    var route: OutputRoute?
    var boundRoutes: [OutputRoute] = []
    var filter: TrackFilter = defaultTrackFilter()
    var folderList: [MusicFolder] = [] {
        didSet {
            guard folderList != oldValue else { return }
            features.downloads.wantsChanged()
        }
    }
    private(set) var currentId: UInt64?
    private(set) var currentQueueId: UInt64?
    private(set) var favouriteIds: Set<UInt64> = []
    @ObservationIgnored private var favouriteWrites: [UInt64: FavouriteWrite] = [:]
    @ObservationIgnored private var favouriteSerial = 0

    @ObservationIgnored private(set) var native: MobilePlayer?
    @ObservationIgnored private var waiting: [CheckedContinuation<MobilePlayer, Swift.Error>] = []
    @ObservationIgnored private var openingFailure: (any Swift.Error)?
    @ObservationIgnored private var opening: [URL] = []
    @ObservationIgnored private var openSettle: Task<Void, Never>?
    @ObservationIgnored private let actions: AsyncStream<NativeAction>
    @ObservationIgnored private let enqueue: AsyncStream<NativeAction>.Continuation
    @ObservationIgnored private var polling: Task<Void, Never>?
    @ObservationIgnored private var scanTask: Task<Void, Never>?
    @ObservationIgnored private var queuedScans: [String] = []
    @ObservationIgnored private var scanningFolder: String?
    @ObservationIgnored private var scanStopped = false
    @ObservationIgnored private var queryRequest = 0
    @ObservationIgnored private var catalogRequest = 0
    @ObservationIgnored private var collectionRequest = 0
    @ObservationIgnored private let preferencesStore = PlaybackPreferencesStore()
    @ObservationIgnored private var catalogIds: Set<UInt64> = []
    @ObservationIgnored private var shownFailure: String?
    @ObservationIgnored private var pollFailures: [PollPart: String] = [:]
    @ObservationIgnored private var statusSerial = 0
    @ObservationIgnored private var fading: Task<Void, Never>?
    @ObservationIgnored private var steering = Settling()
    @ObservationIgnored private var steered = Steered()
    @ObservationIgnored private var queueSettling = Settling()
    @ObservationIgnored let support: URL
    @ObservationIgnored lazy var features = Features(library: self)
    @ObservationIgnored lazy var nowPlaying = NowPlaying(library: self)
    @ObservationIgnored lazy var songSearch = SearchModel(library: self, remembering: true)
    @ObservationIgnored lazy var shelfSearch = SearchModel(library: self, remembering: false)
    @ObservationIgnored lazy var tabSearch = SearchModel(library: self, remembering: true)

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
                route = OutputRoute.current
                boundRoutes = preferencesStore.boundRoutes()
                let stored = preferencesStore.read(for: route)
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
        var listed: QueueListed?
        var unreadable: QueueListed?
        while !Task.isCancelled {
            ticks += 1
            let saving = ticks % Self.ticksBetweenSaves == 0
            let steeringAsked = steering.asked
            let steeringSettled = steering.settled
            let queueAsked = queueSettling.asked
            let queueSettled = queueSettling.settled
            let known = listed
            let refused = unreadable
            do {
                let (state, saved) = try await Work.inOrder {
                    let state = try native.snapshot()
                    let saved: Result<Void, any Swift.Error>? = saving ? Result { try native.save() } : nil
                    return (state, saved)
                }
                let wanted = QueueListed(state)
                let rows = wanted == known || wanted == refused ? nil : await Self.read(queueOf: native)
                let steeringFresh = steeringSettled && steering.asked == steeringAsked
                let queueFresh = queueSettled && queueSettling.asked == queueAsked

                pollFailures[.snapshot] = nil
                publish(steeringFresh ? state : steered.applied(to: state))
                switch rows {
                case .success(let read):
                    pollFailures[.queue] = nil
                    unreadable = nil
                    if queueFresh {
                        if queue != read { queue = read }
                        listed = QueueListed(state)
                    }
                case .failure(let error):
                    reportPolling(.queue, error)
                    unreadable = QueueListed(state)
                case nil:
                    break
                }
                switch saved {
                case .success:
                    pollFailures[.save] = nil
                case .failure(let error):
                    reportPolling(.save, error)
                case nil:
                    break
                }
                nowPlaying.update(state)
                await features.tick(native, state)
            } catch {
                reportPolling(.snapshot, error)
            }
            try? await Task.sleep(for: Self.tick)
        }
    }

    private static func read(queueOf native: MobilePlayer) async -> Result<[Track], any Swift.Error> {
        do {
            return .success(try await Work.aside { try native.queue() })
        } catch {
            return .failure(error)
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
        steady.queueRevision = 0
        steady.catalogRevision = 0
        return steady
    }

    private func reportPolling(_ part: PollPart, _ failure: any Swift.Error) {
        let text = explained(failure)
        guard text != pollFailures[part] else { return }
        pollFailures[part] = text
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

    var playingNext: Range<Int>? {
        guard let first = snapshot?.nextFirst, let count = snapshot?.nextCount, count > 0 else { return nil }
        return Int(first)..<Int(first) + Int(count)
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
            let rows = try await Work.aside { try native.queue() }
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
        favouriteSerial += 1
        let serial = favouriteSerial

        markFavourite(id, favourite)
        favouriteWrites[id] = FavouriteWrite(value: favourite, serial: serial, answeredAfter: nil)
        attempt({ native in try native.favourite(id: id, favourite: favourite) }) { [self] failure in
            guard favouriteWrites[id]?.serial == serial else { return }
            guard let failure else {
                favouriteWrites[id]?.answeredAfter = catalogRequest
                return
            }
            favouriteWrites[id] = nil
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
        let route = route
        request({ native in
            let next = change(native.playbackPreferences())
            try native.configurePlayback(preferences: next)
            store.write(next, for: route)
            return next
        }) { [self] next in preferences = next }
    }

    var equaliserBound: Bool {
        guard let route else { return false }
        return boundRoutes.contains { $0.id == route.id }
    }

    func outputRouteChanged(_ now: OutputRoute?) {
        guard now != route else { return }
        route = now
        followRoute()
    }

    func bindEqualiser(_ bound: Bool) {
        guard let route else { return }
        if bound {
            preferencesStore.bind(route, to: preferences?.equaliser ?? .flat)
        } else {
            preferencesStore.unbind(route)
        }
        boundRoutes = preferencesStore.boundRoutes()
        if !bound { followRoute() }
    }

    func forgetEqualiser(for forgotten: OutputRoute) {
        preferencesStore.unbind(forgotten)
        boundRoutes = preferencesStore.boundRoutes()
        if forgotten.id == route?.id { followRoute() }
    }

    private func followRoute() {
        guard native != nil else { return }
        let curve = preferencesStore.read(for: route).equaliser
        guard curve != preferences?.equaliser else { return }
        updatePreferences { var next = $0; next.equaliser = curve; return next }
    }

    var catalogEmpty: Bool { catalogRead && allTracks.isEmpty && !scanning }
    var filtering: Bool { filter != defaultTrackFilter() }

    func artworkTrack(_ id: UInt64) -> Track? {
        artworkTracks[id]
    }

    func setFilter(_ value: TrackFilter) {
        filter = value
        songSearch.text = ""
        reload(fresh: true)
    }

    func refresh() async {
        guard let native = try? await player() else { return }
        catalogRequest += 1
        let request = catalogRequest
        let everything = defaultTrackFilter()

        reloadCollections()
        if filtering {
            reload()
        } else {
            songSearch.refresh()
            shelfSearch.refresh()
            tabSearch.refresh()
        }

        do {
            let first = try await Work.aside { try native.filteredTracks(filter: everything, offset: 0, limit: Library.page) }
            guard request == catalogRequest else { return }
            if !catalogRead || first.count < Library.page {
                publishCatalog(first, request: request)
            }
            guard first.count == Library.page else { return }

            let rest = try await Work.aside { try Library.pages(native, everything, from: Library.page) }
            guard request == catalogRequest else { return }
            publishCatalog(first + rest, request: request)
        } catch {
            report(error)
        }
    }

    private func settled(_ rows: [Track], request: Int) -> [Track] {
        favouriteWrites = favouriteWrites.filter { ($0.value.answeredAfter ?? .max) >= request }
        guard !favouriteWrites.isEmpty else { return rows }
        return rows.map { row in
            guard let write = favouriteWrites[row.id], write.value != row.favourite else { return row }
            var row = row
            row.favourite = write.value
            return row
        }
    }

    private func publishCatalog(_ read: [Track], request: Int) {
        let rows = settled(read, request: request)
        allTracks = rows
        artworkTracks = Dictionary(rows.map { ($0.id, $0) }, uniquingKeysWith: { kept, _ in kept })
        catalogRead = true
        if !filtering {
            queryRequest += 1
            tracks = rows
            tracksRead = true
        }
    }

    func reload(fresh: Bool = false) {
        songSearch.refresh()
        shelfSearch.refresh()
        tabSearch.refresh()
        guard fresh || filtering else { return }

        queryRequest += 1
        let request = queryRequest
        let query = filter
        Task {
            guard let native = try? await player() else { return }
            do {
                let first = try await Work.aside { try native.filteredTracks(filter: query, offset: 0, limit: Library.page) }
                guard request == queryRequest else { return }
                if fresh || !tracksRead || first.count < Library.page {
                    tracks = first
                    tracksRead = true
                }
                guard first.count == Library.page else { return }

                let rest = try await Work.aside { try Library.pages(native, query, from: Library.page) }
                guard request == queryRequest else { return }
                tracks = first + rest
            } catch {
                report(error)
            }
        }
    }

    func reloadCollections() {
        collectionRequest += 1
        let request = collectionRequest
        let order = collectionOrder

        Task {
            guard let native = try? await player() else { return }
            do {
                let (albums, artists) = try await Work.aside { (try native.albums(order: order), try native.artists(order: order)) }
                guard request == collectionRequest else { return }
                self.albums = albums
                self.artists = artists
                collectionGeneration += 1
            } catch {
                report(error)
            }
        }
    }

    nonisolated private static func pages(_ native: MobilePlayer, _ filter: TrackFilter, from offset: UInt32) throws -> [Track] {
        var found: [Track] = []
        while true {
            let page = try native.filteredTracks(filter: filter, offset: offset + UInt32(found.count), limit: Library.page)
            found += page
            if page.count < Library.page { return found }
        }
    }

    private static func runs(of albums: [AlbumHit], in order: CollectionOrder) -> [LetterRun] {
        switch order {
        case .alphabetical: LetterRun.runs(of: albums, named: \.title)
        case .artist: LetterRun.runs(of: albums, named: \.artist)
        case .year, .added, .plays: []
        }
    }

    private static func runs(of artists: [ArtistHit], in order: CollectionOrder) -> [LetterRun] {
        switch order {
        case .alphabetical, .artist: LetterRun.runs(of: artists, named: \.name)
        case .year, .added, .plays: []
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
            Task { [weak self] in await self?.hideSongs(under: folder.id) }
        } catch {
            report(error)
        }
    }

    private func hideSongs(under folder: String) async {
        guard let native = try? await player() else { return }
        let prefix = "\(Folders.scheme)://\(folder)/"
        guard let changed = try? await Work.aside({ try native.hideSongsUnder(prefix: prefix) }) else { return }
        if changed.leftTheQueue > 0 { await rereadQueue() }
        if changed.hidden > 0 { await refresh() }
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
        if !scanStopped { await forgetMissing() }
        features.scanned()
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

    private static let openedTogether = Duration.milliseconds(250)

    func open(_ url: URL) {
        if url.scheme?.lowercased() == "resonate" {
            features.offerPairing(url.absoluteString)
            return
        }
        if playlistFileExtensions().contains(url.pathExtension.lowercased()) {
            importPlaylistFile(url)
            return
        }
        opening.append(url)
        openSettle?.cancel()
        openSettle = Task { [weak self] in
            try? await Task.sleep(for: Self.openedTogether)
            guard !Task.isCancelled, let self else { return }
            let batch = opening
            opening = []
            await playOpened(batch)
        }
    }

    private func playOpened(_ urls: [URL]) async {
        let folders = folders
        var tracks: [Track] = []
        var failure: (any Swift.Error)?
        do {
            let native = try await player()
            for url in urls {
                do {
                    let kept = try await Work.aside { try Library.kept(url, in: folders) }
                    let indexed = try await Work.aside { try native.indexDocument(uri: kept.uri, name: kept.name) }
                    tracks.append(indexed)
                } catch {
                    failure = error
                }
            }
        } catch {
            failure = error
        }
        if !tracks.isEmpty { play(tracks, at: 0) }
        if let failure { report(failure) }
        await refresh()
    }

    func importPlaylistFile(_ url: URL) {
        Task {
            do {
                let native = try await player()
                let name = url.lastPathComponent
                let imported = try await Work.aside {
                    let scoped = url.startAccessingSecurityScopedResource()
                    defer { if scoped { url.stopAccessingSecurityScopedResource() } }
                    return try native.importPlaylistFile(bytes: try Data(contentsOf: url), fileName: name)
                }
                features.loadPlaylists()
                features.playlistToShow = imported.id
                notice(imported.missing == 0
                    ? "Imported \(imported.name): \(counted(Int(imported.matched), "song"))"
                    : "Imported \(imported.name): \(Int(imported.matched)) of \(Int(imported.matched + imported.missing)) songs found in your library")
            } catch {
                report(error)
            }
        }
    }

    nonisolated private static func kept(_ url: URL, in folders: Folders) throws -> (uri: String, name: String) {
        let scoped = url.startAccessingSecurityScopedResource()
        defer { if scoped { url.stopAccessingSecurityScopedResource() } }
        if let uri = folders.granted(url) {
            return (uri, url.lastPathComponent)
        }
        if let copy = folders.localCopy(identicalTo: url) {
            return copy
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

    @discardableResult
    func forgetMissing() async -> MissingSongs? {
        guard let native = try? await player() else { return nil }
        let prefixes = await folders.refreshed()
            .filter(\.readable)
            .map { "\(Folders.scheme)://\($0.id)/" }
        guard let changed = try? await Work.aside({ try native.hideMissingSongs(prefixes: prefixes) }) else { return nil }
        if changed.leftTheQueue > 0 { await rereadQueue() }
        guard changed.hidden > 0 || changed.restored > 0 else { return changed }
        await refresh()
        if changed.hidden > 0 {
            let fetching = features.syncConfig.map { $0.enabled && ($0.everything || $0.favourites || $0.playlists) } ?? false
            let gone = changed.hidden == 1 ? "1 song whose file is gone was hidden" : "\(changed.hidden) songs whose files are gone were hidden"
            notice(gone + (fetching ? " · fetching from your devices" : ""))
        }
        return changed
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

    func eraseEverything() {
        guard !erasing else { return }
        erasing = true
        let opened = native
        let documents = folders
        Task {
            var songs: [String] = []
            if let opened {
                do {
                    songs = try await Work.inOrder { try opened.songUris() }
                } catch {
                    erasing = false
                    report(error)
                    return
                }
            }
            polling?.cancel()
            queuedScans.removeAll()
            scanTask?.cancel()
            if let scanningFolder {
                scanStopped = true
                documents.stopScanning(scanningFolder)
            }
            await features.stopForErasing()
            if let opened {
                let latest = try? await Work.inOrder { try opened.songUris() }
                songs = latest ?? songs
                _ = try? await Work.inOrder { try opened.shutdown() }
            }
            await AppearanceState.shared.resetIcon()
            let erased = songs
            _ = try? await Work.aside {
                erased.forEach { documents.delete($0) }
                Erasure.everything()
            }
            exit(0)
        }
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
