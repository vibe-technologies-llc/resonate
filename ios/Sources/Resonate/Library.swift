import Accessibility
import Foundation
import Observation
import ResonateLogic
import ResonateNative
import UIKit

typealias NativeAction = @Sendable (MobilePlayer) async -> Void

struct StatusMessage: Equatable {
    enum Kind {
        case notice
        case failure
    }

    enum Offer {
        case openCatalog
    }

    let text: String
    let kind: Kind
    let serial: Int
    var offer: Offer?
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

@MainActor
@Observable
final class FavouriteFlag {
    var on: Bool

    init(_ on: Bool) {
        self.on = on
    }
}

struct FavouriteWrite: Equatable {
    let value: Bool
    let serial: Int
    var answeredAfter: Int?
}

struct CatalogShown: Sendable {
    let rows: [Track]
    let index: [UInt64: Int]
    let favourites: [Track]
    let favouriteIds: Set<UInt64>
    let artwork: [UInt64: Track]

    init(_ rows: [Track]) {
        self.rows = rows
        index = Dictionary(rows.enumerated().map { ($1.id, $0) }, uniquingKeysWith: { kept, _ in kept })
        favourites = rows.filter(\.favourite)
        favouriteIds = Set(favourites.map(\.id))
        artwork = Dictionary(rows.map { ($0.id, $0) }, uniquingKeysWith: { kept, _ in kept })
    }
}

struct CatalogChanges: Sendable {
    let rows: [Track]
    let catalog: CatalogShown?
    let listedRuns: [LetterRun]?

    init(_ read: [Track], writes: [UInt64: Bool], shown: [Track], listed: [Track]?, titled: Bool) {
        let rows = writes.isEmpty ? read : read.map { row in
            guard let value = writes[row.id], value != row.favourite else { return row }
            var row = row
            row.favourite = value
            return row
        }
        self.rows = rows
        catalog = rows == shown ? nil : CatalogShown(rows)
        listedRuns = listed.flatMap { listed in
            guard listed != rows else { return nil }
            return titled ? LetterRun.runs(of: rows, named: \.shownTitle) : []
        }
    }
}

enum PollPart: Hashable {
    case snapshot
    case queue
    case save
}

struct SongsListed: Sendable {
    let rows: [Track]
    let runs: [LetterRun]

    init?(_ rows: [Track], shown: [Track], titled: Bool) {
        guard rows != shown else { return nil }
        self.rows = rows
        runs = titled ? LetterRun.runs(of: rows, named: \.shownTitle) : []
    }
}

struct CollectionsListed: Sendable {
    let albums: [AlbumHit]
    let artists: [ArtistHit]
    let albumRuns: [LetterRun]
    let artistRuns: [LetterRun]
}

final class PlayerWake: PlayerWatch, @unchecked Sendable {
    private weak var library: Library?

    init(_ library: Library) {
        self.library = library
    }

    func moved() {
        let library = library
        Task { @MainActor in library?.wakePoll() }
    }
}

@MainActor
@Observable
final class Library {
    nonisolated static let page: UInt32 = 500
    nonisolated private static let playlistFileBytesAtMost = 8 * 1024 * 1024
    private static let savedEvery: Duration = .seconds(5)
    private static let busyAfterAsking: Duration = .seconds(2)
    private static let noticeLasts: Duration = .seconds(6)
    private static let documentsSettleFor: Duration = .seconds(2)

    nonisolated static var supportFolder: URL {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("Resonate", isDirectory: true)
    }

    let folders: Folders
    let clock = PlayClock()
    private(set) var allTracks: [Track] = []
    var tracks: [Track] = [] {
        didSet {
            guard !marking else { return }
            trackRuns = filter.order == .title ? LetterRun.runs(of: tracks, named: \.shownTitle) : []
        }
    }
    private(set) var trackRuns: [LetterRun] = []
    private(set) var tracksRead = false
    private(set) var catalogRead = false
    private(set) var catalogEpoch = 0
    private(set) var catalogCount = 0
    private(set) var artworkTracks: [UInt64: Track] = [:]
    private(set) var albums: [AlbumHit] = []
    private(set) var albumRuns: [LetterRun] = []
    private(set) var artists: [ArtistHit] = []
    private(set) var artistRuns: [LetterRun] = []
    private(set) var collectionGeneration = 0
    var collectionOrder = CollectionOrder.alphabetical {
        didSet { if collectionOrder != oldValue { reloadCollections() } }
    }
    var queue: [Track] = [] {
        didSet {
            findPlayingRow()
            if queue.count != oldValue.count { nowPlaying.queueChanged() }
        }
    }
    private(set) var playingQueueRow: Int?
    private(set) var snapshot: Snapshot? {
        didSet { derive(from: snapshot) }
    }
    private(set) var transport = Transport.stopped
    private(set) var playingTrack: Track?
    private(set) var loaded = false
    private(set) var playingNext: Range<Int>?
    private(set) var waitingForADevice: String?
    var status: StatusMessage?
    var scanning = false
    private(set) var scanTold = false
    private(set) var erasing = false
    private(set) var erasedLeaving: ErasureLeftovers?
    var scanned = 0
    var preferences: PlaybackPreferences?
    private(set) var volume: Float = 1
    var route: OutputRoute?
    var routeReading = RouteReading.unread
    var boundRoutes: [OutputRoute] = []
    nonisolated static let everything = defaultTrackFilter()
    var filter: TrackFilter = Library.everything
    var folderList: [MusicFolder] = [] {
        didSet {
            guard folderList != oldValue else { return }
            features.downloads.wantsChanged()
        }
    }
    private(set) var currentId: UInt64?
    private(set) var currentQueueId: UInt64? {
        didSet { findPlayingRow() }
    }
    @ObservationIgnored private var favouriteIds: Set<UInt64> = []
    @ObservationIgnored private var favouriteFlags: [UInt64: FavouriteFlag] = [:]
    private(set) var favouriteTracks: [Track] = [] {
        didSet { favouriteEpoch &+= 1 }
    }
    private(set) var favouriteEpoch = 0
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
    @ObservationIgnored private var queueReading: Task<Void, Never>?
    @ObservationIgnored private var queueListed: QueueListed?
    @ObservationIgnored private var queueUnreadable: QueueListed?
    @ObservationIgnored private var queueReadAt = ContinuousClock.now
    @ObservationIgnored private var volumeSending = false
    @ObservationIgnored private var volumeOwed: Float?
    @ObservationIgnored private var refreshing: Task<Void, Never>?
    @ObservationIgnored private var refreshOwed = false
    @ObservationIgnored private var scanTask: Task<Void, Never>?
    @ObservationIgnored private var catalogWhole = false
    @ObservationIgnored private var catalogWaiting: [CheckedContinuation<Void, Never>] = []
    @ObservationIgnored private var queuedScans: [QueuedScan] = []
    @ObservationIgnored private var unaskedRescan: Task<Void, Never>?
    @ObservationIgnored private var owedRescan: RescanScope?
    @ObservationIgnored private var unaskedScanFailure: String?
    @ObservationIgnored private var documentsWatch: DocumentsWatch?
    @ObservationIgnored private var documentsSettle: Task<Void, Never>?
    @ObservationIgnored private var scanningFolder: String?
    @ObservationIgnored private var scanStopped = false
    @ObservationIgnored private var queryRequest = 0
    @ObservationIgnored private var catalogRequest = 0
    @ObservationIgnored private var collectionRequest = 0
    @ObservationIgnored private let preferencesStore = PlaybackPreferencesStore()
    @ObservationIgnored private var catalogIndex: [UInt64: Int] = [:]
    @ObservationIgnored private var marking = false
    @ObservationIgnored private var shownFailure: String?
    @ObservationIgnored private var hidingFailure: String?
    @ObservationIgnored private var pollFailures: [PollPart: String] = [:]
    @ObservationIgnored private var statusSerial = 0
    @ObservationIgnored private var fading: Task<Void, Never>?
    @ObservationIgnored private var steering = Settling()
    @ObservationIgnored private var steered = Steered()
    @ObservationIgnored private var queueSettling = Settling()
    @ObservationIgnored let support: URL
    @ObservationIgnored private var pollSleep: Task<Void, Never>?
    @ObservationIgnored private var wakeOwed = false
    @ObservationIgnored private var busyUntil = ContinuousClock.now
    @ObservationIgnored lazy var features = Features(library: self)
    @ObservationIgnored lazy var nowPlaying = NowPlaying(library: self)
    @ObservationIgnored lazy var songSearch = SearchModel(library: self, remembering: true)
    @ObservationIgnored lazy var shelfSearch = SearchModel(library: self, remembering: false)
    @ObservationIgnored lazy var tabSearch = SearchModel(library: self, remembering: true)

    init() {
        support = Self.supportFolder
        folders = Folders(support: support)
        (actions, enqueue) = AsyncStream.makeStream(of: NativeAction.self)
        Task { [weak self] in await self?.serveActions() }
    }

    func start() {
        guard polling == nil else { return }
        let documents = folders
        let support = support
        Task.detached(priority: .utility) { Backup.leaveOutDerived(in: support) }
        Task { [weak self] in
            let measured = await documents.refreshed()
            self?.folderList = measured
        }
        open()
    }

    func openAgain() {
        guard native == nil, openingFailure != nil, !erasing else { return }
        if status?.offer == .openCatalog { dismissStatus() }
        open()
    }

    private func open() {
        let folder = support.path
        let documents = folders
        let support = support
        openingFailure = nil
        volumeSending = false
        volumeOwed = nil
        polling = Task { [weak self] in
            do {
                async let opening = Work.inOrder {
                    try MobilePlayer(folder: folder, documents: documents)
                }
                async let routed = Work.inSession { OutputRoute.current }
                let opened = try await opening
                let current = await routed
                guard let self else { return }
                route = current
                boundRoutes = preferencesStore.boundRoutes()
                let stored = preferencesStore.read(for: route)
                try await Work.inOrder { try opened.configurePlayback(preferences: stored) }
                preferences = stored
                volume = preferencesStore.volume
                if volume < 1 {
                    let kept = volume
                    try await Work.inOrder { try opened.setVolume(volume: kept) }
                }
                native = opened
                opened.watchPlayer(watch: PlayerWake(self))
                Task.detached(priority: .utility) { [support] in Backup.leaveOutDerived(in: support) }
                waiting.forEach { $0.resume(returning: opened) }
                waiting.removeAll()
                nowPlaying.start(opened)
                features.initialize(opened)
                watchDocuments()
                Task {
                    await refresh()
                    if UIApplication.shared.applicationState != .background { rescanUnasked(.everything) }
                }
                await poll(opened)
            } catch {
                guard let self else { return }
                openingFailure = error
                waiting.forEach { $0.resume(throwing: error) }
                waiting.removeAll()
                show(explained(error), as: .failure, offering: .openCatalog)
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
        var lastSaved = ContinuousClock.now
        queueListed = nil
        queueUnreadable = nil
        while !Task.isCancelled {
            var wait = Polling.busy
            let steeringAsked = steering.asked
            let steeringSettled = steering.settled
            if lastSaved.duration(to: .now) >= Self.savedEvery {
                lastSaved = .now
                save(native)
            }
            wakeOwed = false
            do {
                let state = try await Work.inOrder { try native.snapshot() }
                wait = Polling.interval(
                    foreground: features.foreground,
                    state: state,
                    analysing: features.analysisBusy,
                    asked: ContinuousClock.now < busyUntil
                )
                let steeringFresh = steeringSettled && steering.asked == steeringAsked

                pollFailures[.snapshot] = nil
                if steeringFresh { steered = Steered() }
                publish(steeringFresh ? state : steered.applied(to: state))
                listQueue(of: native, as: state)
                nowPlaying.update(state)
                await features.tick(native, state)
            } catch {
                reportPolling(.snapshot, error)
            }
            await sleepBetweenPolls(wait)
        }
    }

    private func save(_ native: MobilePlayer) {
        Task {
            do {
                try await Work.inSaving { try native.save() }
                pollFailures[.save] = nil
            } catch {
                reportPolling(.save, error)
            }
        }
    }

    private func listQueue(of native: MobilePlayer, as state: Snapshot) {
        let wanted = QueueListed(state)
        guard queueReading == nil, wanted.isDue(after: queueUnreadable ?? queueListed, since: queueReadAt) else { return }
        let asked = queueSettling.asked
        let settled = queueSettling.settled
        let shown = queue

        queueReading = Task {
            let rows = await Self.read(queueOf: native, shown: shown)
            queueReading = nil
            switch rows {
            case .success(let read):
                pollFailures[.queue] = nil
                queueUnreadable = nil
                guard settled, queueSettling.asked == asked else {
                    nudgePoll()
                    return
                }
                if let read { queue = read }
                queueListed = wanted
                queueReadAt = .now
            case .failure(let error):
                reportPolling(.queue, error)
                queueUnreadable = wanted
                queueReadAt = .now
            }
        }
    }

    func nudgePoll() {
        busyUntil = ContinuousClock.now + Self.busyAfterAsking
        pollSleep?.cancel()
    }

    func wakePoll() {
        wakeOwed = true
        pollSleep?.cancel()
    }

    private func sleepBetweenPolls(_ wait: Duration) async {
        if wakeOwed { return }
        let sleeper = Task<Void, Never> { try? await Task.sleep(for: wait) }
        pollSleep = sleeper
        await withTaskCancellationHandler {
            await sleeper.value
        } onCancel: {
            sleeper.cancel()
        }
    }

    private static func read(queueOf native: MobilePlayer, shown: [Track]) async -> Result<[Track]?, any Swift.Error> {
        do {
            return .success(try await Work.onScreen {
                let read = try native.queue()
                return Library.sameRows(read, shown) ? nil : read
            })
        } catch {
            return .failure(error)
        }
    }

    nonisolated static func sameRows(_ read: [Track], _ shown: [Track]) -> Bool {
        read.count == shown.count && zip(read, shown).allSatisfy { read, shown in
            guard read.favourite != shown.favourite else { return read == shown }
            var read = read
            read.favourite = shown.favourite
            return read == shown
        }
    }

    private func publish(_ state: Snapshot) {
        clock.follow(state)
        if snapshot.map(Polling.steady) != Polling.steady(state) { snapshot = state }
        if currentId != state.current?.id {
            currentId = state.current?.id
            shownFailure = nil
        }
        if currentQueueId != state.current?.queueId { currentQueueId = state.current?.queueId }
        if let failure = state.failure, failure != shownFailure {
            shownFailure = failure
            show(failure, as: .failure)
        }
        if waitingForADevice != state.waiting {
            if let ended = waitingForADevice, status?.text == ended { dismissStatus() }
            waitingForADevice = state.waiting
            if let begun = state.waiting { show(begun, as: .failure) }
        }
    }

    private func reportPolling(_ part: PollPart, _ failure: any Swift.Error) {
        let text = explained(failure)
        guard text != pollFailures[part] else { return }
        pollFailures[part] = text
        show(text, as: .failure)
    }

    func request(_ action: @escaping @Sendable (MobilePlayer) throws -> Void) {
        nudgePoll()
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


    private func derive(from state: Snapshot?) {
        let shownTransport = state?.transport ?? .stopped
        let next = state.flatMap(Self.askedNext(in:))

        if transport != shownTransport {
            transport = shownTransport
            features.tapHoldersChanged()
        }
        if playingTrack != state?.current {
            playingTrack = state?.current
            if let track = playingTrack { Task { await BackdropView.prepare(track, features: features) } }
        }
        if loaded != (playingTrack != nil) { loaded = playingTrack != nil }
        if playingNext != next { playingNext = next }
    }

    private static func askedNext(in state: Snapshot) -> Range<Int>? {
        guard let first = state.nextFirst, state.nextCount > 0 else { return nil }
        return Int(first)..<Int(first) + Int(state.nextCount)
    }

    private func findPlayingRow() {
        let found = currentQueueId.flatMap { current in queue.firstIndex { $0.queueId == current } }
        if found != playingQueueRow { playingQueueRow = found }
    }

    func isFavourite(_ track: Track) -> Bool {
        favouriteFlag(of: track).on
    }

    private func favouriteFlag(of track: Track) -> FavouriteFlag {
        if let kept = favouriteFlags[track.id] { return kept }
        let made = FavouriteFlag(catalogIndex[track.id] != nil ? favouriteIds.contains(track.id) : track.favourite)
        favouriteFlags[track.id] = made
        return made
    }

    private func flagFavourites() {
        for (id, flag) in favouriteFlags where catalogIndex[id] != nil {
            let on = favouriteIds.contains(id)
            if flag.on != on { flag.on = on }
        }
    }

    func track(_ id: UInt64) -> Track? {
        if let at = catalogIndex[id] { return allTracks[at] }
        if let playingTrack, playingTrack.id == id { return playingTrack }
        return queue.first { $0.id == id }
            ?? features.playlistRows.first { $0.id == id }
            ?? features.statistics?.topTracks.first { $0.id == id }
    }

    private func adopt(_ catalog: CatalogShown) {
        catalogEpoch += 1
        catalogIndex = catalog.index
        allTracks = catalog.rows
        artworkTracks = catalog.artwork
        if catalogCount != catalog.rows.count { catalogCount = catalog.rows.count }
        if favouriteIds != catalog.favouriteIds {
            favouriteIds = catalog.favouriteIds
            flagFavourites()
        }
        if favouriteTracks != catalog.favourites { favouriteTracks = catalog.favourites }
    }

    private func keepFavourite(_ track: Track, at place: Int) {
        favouriteFlags[track.id]?.on = track.favourite
        guard track.favourite else {
            favouriteIds.remove(track.id)
            if let at = favouriteTracks.firstIndex(where: { $0.id == track.id }) { favouriteTracks.remove(at: at) }
            return
        }
        favouriteIds.insert(track.id)
        var low = 0
        var high = favouriteTracks.count
        while low < high {
            let middle = (low + high) / 2
            if (catalogIndex[favouriteTracks[middle].id] ?? 0) < place { low = middle + 1 } else { high = middle }
        }
        favouriteTracks.insert(track, at: low)
    }

    func play(_ rows: [Track], at index: Int, shuffle: Bool = false) {
        guard !rows.isEmpty else { return }
        let ids = rows.map(\.id)
        let start = UInt32(min(max(index, 0), rows.count - 1))

        nowPlaying.activate(loading: rows[Int(start)])
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
        nowPlaying.activate(loading: row)
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
        steerSong(by: 1) { try $0.next() }
    }

    func previous() {
        guard !Steering.previousRestarts(positionMs: clock.positionMs, durationMs: clock.durationMs) else {
            steer({ $0.song = nil; $0.restarted = true }) { try $0.previous() }
            return
        }
        steerSong(by: -1) { try $0.previous() }
    }

    private func steerSong(by step: Int, _ action: @escaping @Sendable (MobilePlayer) throws -> Void) {
        guard let song = neighbour(step) else {
            request(action)
            return
        }
        steer({ $0.song = song; $0.restarted = false }, action)
    }

    private func neighbour(_ step: Int) -> SteeredSong? {
        Steering.neighbour(of: playingQueueRow, step: step, in: queue, repeating: repeatMode != .off)
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
        nudgePoll()
        change(&steered)
        if let shown = snapshot {
            let expected = steered.applied(to: shown)
            if expected != shown { showSteered(expected) }
        }
        attempt(action) { [self] failure in
            steering.answer()
            if let failure { report(failure) }
        }
    }

    private func showSteered(_ expected: Snapshot) {
        snapshot = expected
        clock.follow(expected)
        if currentId != expected.current?.id {
            currentId = expected.current?.id
            shownFailure = nil
        }
        if currentQueueId != expected.current?.queueId { currentQueueId = expected.current?.queueId }
        nowPlaying.update(expected)
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
        let shown = queue
        do {
            let rows = try await Work.onScreen {
                let read = try native.queue()
                return Library.sameRows(read, shown) ? nil : read
            }
            if queueSettling.settled, let rows { queue = rows }
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
                Task { await rereadQueue() }
            } else if let done {
                notice(done)
            }
            nudgePoll()
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
        func marked(_ rows: [Track]) -> [Track]? {
            guard rows.contains(where: { $0.id == id && $0.favourite != favourite }) else { return nil }
            return rows.map { row in
                var row = row
                if row.id == id { row.favourite = favourite }
                return row
            }
        }

        if let at = catalogIndex[id], favouriteIds.contains(id) != favourite {
            var marked = allTracks[at]
            marked.favourite = favourite
            keepFavourite(marked, at: at)
        }
        if let flag = favouriteFlags[id], flag.on != favourite { flag.on = favourite }
        if filter.favourites, !favourite { tracks = tracks.filter { $0.id != id } }
        if catalogIndex[id] == nil, let rows = marked(queue) { queue = rows }
        if let rows = marked(features.playlistRows) { features.playlistRows = rows }
        if let top = features.statistics?.topTracks, let rows = marked(top) { features.statistics?.topTracks = rows }
        if features.inspector?.track.id == id { features.inspector?.track.favourite = favourite }
        guard snapshot?.current?.id == id else { return }
        snapshot?.current?.favourite = favourite
        nowPlaying.favouriteChanged()
    }

    func setVolume(_ level: Float) {
        volume = level
        guard !volumeSending else {
            volumeOwed = level
            return
        }
        send(volume: level)
    }

    private func send(volume level: Float) {
        volumeSending = true
        attempt({ try $0.setVolume(volume: level) }) { [self] failure in
            volumeSending = false
            if let failure { report(failure) }
            if let owed = volumeOwed {
                volumeOwed = nil
                send(volume: owed)
            } else {
                preferencesStore.volume = level
            }
        }
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

    var scanShown: Bool { scanning && (scanTold || scanned > 0) }

    var catalogEmpty: Bool { catalogRead && catalogCount == 0 && !scanning }
    var filtering: Bool { filter != Self.everything }

    func artworkTrack(_ id: UInt64) -> Track? {
        artworkTracks[id]
    }

    func setFilter(_ value: TrackFilter) {
        filter = value
        songSearch.text = ""
        reload(fresh: true)
    }

    func refresh() async {
        if let refreshing {
            refreshOwed = true
            await refreshing.value
            return
        }
        let reading = Task {
            repeat {
                refreshOwed = false
                await readCatalog()
            } while refreshOwed
            refreshing = nil
        }
        refreshing = reading
        await reading.value
    }

    private func readCatalog() async {
        guard let native = try? await player() else { return }
        catalogRequest += 1
        let request = catalogRequest
        let everything = Self.everything

        reloadCollections()
        if filtering {
            reload()
        } else {
            songSearch.refresh()
            shelfSearch.refresh()
            tabSearch.refresh()
        }

        do {
            let first = try await Work.onScreen { try native.filteredTracks(filter: everything, offset: 0, limit: Library.page) }
            guard request == catalogRequest else { return }
            if !catalogRead || first.count < Library.page {
                await publishCatalog(first, request: request)
            }
            guard first.count == Library.page else {
                caughtUpWithCatalog()
                return
            }

            let whole = try await Work.onScreen { first + (try Library.pages(native, everything, from: Library.page)) }
            guard request == catalogRequest else { return }
            await publishCatalog(whole, request: request)
            caughtUpWithCatalog()
        } catch {
            report(error)
        }
    }

    private func caughtUpWithCatalog() {
        catalogWhole = true
        let waiting = catalogWaiting
        catalogWaiting.removeAll()
        waiting.forEach { $0.resume() }
    }

    func wholeCatalog() async {
        guard !catalogWhole else { return }
        await withCheckedContinuation { catalogWaiting.append($0) }
    }

    func gone(among destinations: [Destination]) async -> Set<Destination> {
        guard let native = try? await player() else { return [] }
        await wholeCatalog()
        var gone: Set<Destination> = []
        var asked: [Destination] = []
        for destination in destinations {
            switch destination {
            case .songLyrics(let id?), .songAnalysis(let id?), .songDetails(let id):
                if track(id) == nil { gone.insert(destination) }
            case .album, .artist, .playlist:
                asked.append(destination)
            default:
                break
            }
        }
        let looked = asked
        let missing = (try? await Work.aside { try looked.filter { try !Library.resolves($0, in: native) } }) ?? []
        return gone.union(missing)
    }

    nonisolated private static func resolves(_ destination: Destination, in native: MobilePlayer) throws -> Bool {
        switch destination {
        case .album(let key): try native.album(key: key) != nil
        case .artist(let name): try !native.artistTracks(name: name).isEmpty
        case .playlist(let id): try native.playlists().contains { $0.id == id }
        default: true
        }
    }

    private func publishCatalog(_ read: [Track], request: Int) async {
        let asked = catalogChanges(read, request: request)
        let writes = favouriteWrites

        guard var changes = try? await Work.onScreen(asked), request == catalogRequest else { return }
        if favouriteWrites != writes {
            guard let again = try? await Work.onScreen(catalogChanges(read, request: request)), request == catalogRequest else { return }
            changes = again
        }
        if let catalog = changes.catalog { adopt(catalog) }
        catalogRead = true
        guard !filtering else { return }

        queryRequest += 1
        if let runs = changes.listedRuns {
            marking = true
            tracks = changes.rows
            marking = false
            trackRuns = runs
        }
        tracksRead = true
    }

    private func catalogChanges(_ read: [Track], request: Int) -> @Sendable () -> CatalogChanges {
        favouriteWrites = favouriteWrites.filter { ($0.value.answeredAfter ?? .max) >= request }
        let writes = favouriteWrites.mapValues(\.value)
        let shown = allTracks
        let listed = filtering ? nil : tracks
        let titled = filter.order == .title
        return { CatalogChanges(read, writes: writes, shown: shown, listed: listed, titled: titled) }
    }

    func reload(fresh: Bool = false) {
        songSearch.refresh()
        shelfSearch.refresh()
        tabSearch.refresh()
        guard fresh || filtering else { return }

        queryRequest += 1
        let request = queryRequest
        let query = filter
        let titled = query.order == .title
        Task {
            guard let native = try? await player() else { return }
            do {
                let shown = tracks
                let (first, firstListed) = try await Work.onScreen {
                    let first = try native.filteredTracks(filter: query, offset: 0, limit: Library.page)
                    return (first, SongsListed(first, shown: shown, titled: titled))
                }
                guard request == queryRequest else { return }
                if fresh || !tracksRead || first.count < Library.page {
                    if let firstListed { list(firstListed) }
                    tracksRead = true
                }
                guard first.count == Library.page else { return }

                let listedBefore = tracks
                let whole = try await Work.onScreen {
                    SongsListed(first + (try Library.pages(native, query, from: Library.page)), shown: listedBefore, titled: titled)
                }
                guard request == queryRequest else { return }
                if let whole { list(whole) }
            } catch {
                report(error)
            }
        }
    }

    private func list(_ listed: SongsListed) {
        marking = true
        tracks = listed.rows
        marking = false
        trackRuns = listed.runs
    }

    func reloadCollections() {
        collectionRequest += 1
        let request = collectionRequest
        let order = collectionOrder

        Task {
            guard let native = try? await player() else { return }
            do {
                let listed = try await Work.onScreen {
                    let albums = try native.albums(order: order)
                    let artists = try native.artists(order: order)
                    return CollectionsListed(
                        albums: albums,
                        artists: artists,
                        albumRuns: Library.runs(of: albums, in: order),
                        artistRuns: Library.runs(of: artists, in: order)
                    )
                }
                guard request == collectionRequest else { return }
                albums = listed.albums
                artists = listed.artists
                albumRuns = listed.albumRuns
                artistRuns = listed.artistRuns
                collectionGeneration += 1
            } catch {
                report(error)
            }
        }
    }

    nonisolated private static func pages(_ native: MobilePlayer, _ filter: TrackFilter, from offset: UInt32) throws -> [Track] {
        try everyPage { try native.filteredTracks(filter: filter, offset: offset + $0, limit: Library.page) }
    }

    nonisolated static func everyPage(_ page: (UInt32) throws -> [Track]) rethrows -> [Track] {
        var found: [Track] = []
        while true {
            let read = try page(UInt32(found.count))
            found += read
            if read.count < Library.page { return found }
        }
    }

    nonisolated private static func runs(of albums: [AlbumHit], in order: CollectionOrder) -> [LetterRun] {
        switch order {
        case .alphabetical: LetterRun.runs(of: albums, named: \.title)
        case .artist: LetterRun.runs(of: albums, named: \.artist)
        case .year, .added, .plays: []
        }
    }

    nonisolated private static func runs(of artists: [ArtistHit], in order: CollectionOrder) -> [LetterRun] {
        switch order {
        case .alphabetical, .artist: LetterRun.runs(of: artists, named: \.name)
        case .year, .added, .plays: []
        }
    }

    func addFolder(_ url: URL) {
        let folders = folders
        Task { [weak self] in
            do {
                let id = try await Work.aside { try folders.add(url) }
                await self?.remeasureFolders()
                self?.scan(id)
            } catch {
                self?.report(error)
            }
        }
    }

    func removeFolder(_ folder: MusicFolder) {
        queuedScans.removeAll { $0.folder == folder.id }
        if scanningFolder == folder.id { scanStopped = true }
        let folders = folders
        Task { [weak self] in
            do {
                try await Work.aside { try folders.remove(folder.id) }
                await self?.remeasureFolders()
                await self?.hideSongs(under: folder.id)
            } catch {
                self?.report(error)
            }
        }
    }

    func remeasureFolders() async {
        let folders = folders
        if let measured = try? await Work.aside({ folders.folders() }) { folderList = measured }
    }

    private func hideSongs(under folder: String) async {
        guard let native = try? await player() else { return }
        let prefix = "\(Folders.scheme)://\(folder)/"
        let changed: MissingSongs
        do {
            changed = try await Work.aside { try native.hideSongsUnder(prefix: prefix) }
        } catch {
            report(error)
            return
        }
        if changed.leftTheQueue > 0 { await rereadQueue() }
        if changed.hidden > 0 {
            await refresh()
            await features.reloadOpenPlaylist()
        }
    }

    func rescan() {
        Task { [weak self] in
            await self?.remeasureFolders()
            guard let self else { return }
            for folder in folderList where folder.readable {
                scan(folder.id)
            }
        }
    }

    func stopScanning() {
        queuedScans.removeAll()
        owedRescan = nil
        guard let scanningFolder else { return }
        scanStopped = true
        folders.stopScanning(scanningFolder)
    }

    func scan(_ id: String) {
        guard !queuedScans.contains(QueuedScan(folder: id, asked: true)), scanningFolder != id || !scanTold else { return }
        if let unasked = queuedScans.firstIndex(of: QueuedScan(folder: id, asked: false)) {
            queuedScans[unasked] = QueuedScan(folder: id, asked: true)
            return
        }
        chainScan(QueuedScan(folder: id, asked: true))
    }

    func rescanUnasked(_ scope: RescanScope) {
        guard !erasing, catalogRead else { return }
        guard unaskedRescan == nil else {
            owedRescan = owedRescan.map { $0.joined(with: scope) } ?? scope
            return
        }
        unaskedRescan = Task { [weak self] in
            var next: RescanScope? = scope
            while let scope = next, !Task.isCancelled {
                guard let self else { return }
                await rescanFolders(scope)
                next = owedRescan
                owedRescan = nil
            }
            self?.unaskedRescan = nil
        }
    }

    private func rescanFolders(_ scope: RescanScope) async {
        let roots: [String]
        switch scope {
        case .everything:
            await remeasureFolders()
            roots = folderList.filter(\.readable).map(\.id)
        case .documents:
            roots = [Folders.local]
        }
        for root in roots where !queuedScans.contains(where: { $0.folder == root }) {
            chainScan(QueuedScan(folder: root, asked: false))
        }
        await scanTask?.value
    }

    private func chainScan(_ queued: QueuedScan) {
        queuedScans.append(queued)
        let previous = scanTask
        scanTask = Task { [weak self] in
            await previous?.value
            await self?.walk(queued.folder)
        }
    }

    private func watchDocuments() {
        documentsWatch = DocumentsWatch { [weak self] in
            Task { @MainActor in self?.documentsChanged() }
        }
    }

    private func documentsChanged() {
        guard features.foreground, !erasing else { return }
        documentsSettle?.cancel()
        documentsSettle = Task { [weak self] in
            try? await Task.sleep(for: Self.documentsSettleFor)
            guard !Task.isCancelled else { return }
            self?.rescanUnasked(.documents)
        }
    }

    func afterScans(_ job: @escaping @MainActor () async -> Void) {
        let previous = scanTask
        scanTask = Task {
            await previous?.value
            guard !Task.isCancelled else { return }
            await job()
        }
    }

    private func walk(_ id: String) async {
        guard let queued = queuedScans.firstIndex(where: { $0.folder == id }) else { return }
        let asked = queuedScans.remove(at: queued).asked
        guard let native = try? await player() else { return }
        let folders = folders
        let tally = Tally()
        let shown = asked ? nil : allTracks

        scanning = true
        scanTold = asked
        scanned = 0
        scanStopped = false
        scanningFolder = id
        defer {
            scanningFolder = nil
            scanning = false
            scanTold = false
        }
        var failure: (any Swift.Error)?
        do {
            try await Work.aside { [weak self] in
                let known = shown.map(KnownDocuments.init)
                let covers = FolderCovers(folders: folders, native: native, tally: tally)
                defer { covers.flush() }
                try folders.scan(id, skipped: { path, reason in tally.skipped(path, reason) }) { document in
                    do {
                        let track = try native.scanDocument(
                            uri: document.uri,
                            name: document.name,
                            size: document.size,
                            modifiedMs: document.modifiedMs
                        )
                        if let known, known.holds(track) { return }
                        if let read = tally.indexed() { Task { @MainActor in self?.reached(read) } }
                        covers.add(track.id, cover: document.cover, named: document.name)
                        if let lyrics = document.lyrics, let bytes = folders.companion(lyrics, sidecar: true) {
                            do {
                                try native.keepSidecarLyrics(id: track.id, bytes: bytes)
                            } catch {
                                tally.lost(.lyrics, document.name, error)
                            }
                        }
                    } catch {
                        tally.failed(document.name, error)
                    }
                }
            }
        } catch {
            failure = error
        }
        guard asked else {
            await settleUnasked(tally, failure: failure)
            return
        }
        await refresh()
        await features.reloadOpenPlaylist()
        if !scanStopped, failure == nil, tally.complete { await forgetMissing() }
        features.scanned()
        await remeasureFolders()
        if let failure {
            report(failure)
        } else {
            notice(scanStopped ? tally.stoppedSummary : tally.summary)
        }
    }

    private func settleUnasked(_ tally: Tally, failure: (any Swift.Error)?) async {
        if tally.count > 0 {
            await refresh()
            await features.reloadOpenPlaylist()
            features.scanned()
        }
        await remeasureFolders()
        if let failure {
            let text = explained(failure)
            if text != unaskedScanFailure { report(failure) }
            unaskedScanFailure = text
            return
        }
        unaskedScanFailure = nil
        if scanStopped {
            notice(tally.stoppedSummary)
        } else if tally.count > 0 {
            notice(tally.foundSummary)
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
        var playing = false
        var failure: (any Swift.Error)?
        do {
            let native = try await player()
            for url in urls {
                do {
                    let kept = try await Work.aside { try Library.kept(url, in: folders) }
                    let indexed = try await Work.aside { try native.indexDocument(uri: kept.uri, name: kept.name) }
                    if playing {
                        let id = indexed.id
                        editQueue { try $0.addToQueue(ids: [id]) }
                    } else {
                        playing = true
                        play([indexed], at: 0)
                    }
                } catch {
                    failure = error
                }
            }
        } catch {
            failure = error
        }
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
                    let file = try FileHandle(forReadingFrom: url)
                    defer { try? file.close() }
                    let bytes = try file.read(upToCount: Library.playlistFileBytesAtMost + 1) ?? Data()
                    return try native.importPlaylistFile(bytes: bytes, fileName: name)
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
        folders.written(uri)
        let name = (uri as NSString).lastPathComponent.removingPercentEncoding ?? url.lastPathComponent
        return (uri, name)
    }

    @discardableResult
    func forgetMissing() async -> MissingSongs? {
        guard let native = try? await player() else { return nil }
        let prefixes = await folders.refreshed()
            .filter(\.readable)
            .map { "\(Folders.scheme)://\($0.id)/" }
        let changed: MissingSongs
        do {
            changed = try await Work.aside { try native.hideMissingSongs(prefixes: prefixes) }
            hidingFailure = nil
        } catch {
            let text = explained(error)
            if text != hidingFailure { report(error) }
            hidingFailure = text
            return nil
        }
        if changed.leftTheQueue > 0 { await rereadQueue() }
        guard changed.hidden > 0 || changed.restored > 0 else { return changed }
        await refresh()
        await features.reloadOpenPlaylist()
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

    func report(_ failure: any Swift.Error, after done: String) {
        show("\(done); \(explained(failure))", as: .failure)
    }

    func dismissStatus() {
        fading?.cancel()
        status = nil
    }

    func perform(_ offer: StatusMessage.Offer) {
        switch offer {
        case .openCatalog: openAgain()
        }
    }

    private func show(_ text: String, as kind: StatusMessage.Kind, offering offer: StatusMessage.Offer? = nil) {
        if kind == .notice, status?.kind == .failure { return }
        statusSerial += 1
        let shown = StatusMessage(text: text, kind: kind, serial: statusSerial, offer: offer)

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
        let holding = UIApplication.shared.beginBackgroundTask(withName: "Erasing your data")
        Task {
            var songs: [String] = []
            if let opened {
                do {
                    songs = try await Work.inOrder { try opened.songUris() }
                } catch {
                    erasing = false
                    UIApplication.shared.endBackgroundTask(holding)
                    report(error)
                    return
                }
            }
            polling?.cancel()
            queuedScans.removeAll()
            owedRescan = nil
            unaskedRescan?.cancel()
            documentsSettle?.cancel()
            documentsWatch = nil
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
            let leftovers = (try? await Work.aside {
                let stuck = erased.filter { !documents.delete($0) }
                var leftovers = Erasure.everything(keeping: stuck.isEmpty ? [] : Folders.kept)
                leftovers.songs = stuck.count
                return leftovers
            }) ?? ErasureLeftovers(files: 1)
            guard !leftovers.isNone else { exit(0) }
            erasedLeaving = leftovers
            UIApplication.shared.endBackgroundTask(holding)
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

struct QueuedScan: Equatable {
    let folder: String
    let asked: Bool
}

enum RescanScope {
    case everything
    case documents

    func joined(with other: RescanScope) -> RescanScope {
        self == .documents && other == .documents ? .documents : .everything
    }
}

struct KnownDocuments {
    private let byUri: [String: Track]

    init(_ rows: [Track]) {
        byUri = Dictionary(rows.map { ($0.uri, $0) }, uniquingKeysWith: { kept, _ in kept })
    }

    func holds(_ track: Track) -> Bool {
        guard var known = byUri[track.uri] else { return false }
        known.favourite = track.favourite
        known.queueId = track.queueId
        return known == track
    }
}

final class FolderCovers {
    private let folders: Folders
    private let native: MobilePlayer
    private let tally: Tally
    private var cover: String?
    private var firstSong = ""
    private var songs: [UInt64] = []

    init(folders: Folders, native: MobilePlayer, tally: Tally) {
        self.folders = folders
        self.native = native
        self.tally = tally
    }

    func add(_ song: UInt64, cover next: String?, named name: String) {
        if next != cover {
            flush()
            cover = next
            firstSong = name
        }
        if next != nil { songs.append(song) }
    }

    func flush() {
        defer { songs = [] }
        guard let cover, !songs.isEmpty, let bytes = folders.companion(cover, sidecar: false) else { return }
        do {
            try native.keepFolderCover(ids: songs, bytes: bytes)
        } catch {
            tally.lost(.cover, firstSong, error)
        }
    }
}

