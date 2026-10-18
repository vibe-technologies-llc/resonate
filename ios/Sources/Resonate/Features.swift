import Foundation
import Network
import Observation
import ResonateNative
import UIKit

struct PlaylistSync: Equatable {
    let playlist: UInt64
    let missing: UInt32
    let kept: Bool
}

enum SyncActivity: Equatable {
    case syncing
    case fetching(songs: Int, fraction: Double?)
}

enum ArtistLookup {
    case idle
    case looking
    case found(ArtistInformation)
    case missing
}

typealias PlaylistEdited = @MainActor (String?) -> Void

struct WantedSaving: Sendable {
    let saved: UInt64
    let failure: (any Swift.Error)?
}

@MainActor
@Observable
final class Features {
    private static let landingJournalKey = "unfinished-sync-landing"
    private static let scrobbleEvery: Duration = .seconds(30)
    private static let syncForeground: Duration = .seconds(2)
    private static let syncBackground: Duration = .seconds(30)
    private static let syncAsked: Duration = .seconds(1)
    private static let syncAnswersAfter: TimeInterval = 1.5
    private static let syncGivesUpAfter: TimeInterval = 45
    private static let landingPause: TimeInterval = 60
    private static let landingTriesAtMost = 3
    private static let visualizerFrame: Duration = .milliseconds(33)
    private static let loudnessFrame: Duration = .milliseconds(50)
    private static let visualizerRetry: Duration = .seconds(2)
    private static let artGenerationsAtMost = 4096
    private static let artStampsAtMost = 20_000
    nonisolated private static let wantedChunk = 500

    @ObservationIgnored private unowned let library: Library
    @ObservationIgnored private let accounts = AccountStore()
    @ObservationIgnored private let settings = UserDefaults(suiteName: "library-features") ?? .standard
    @ObservationIgnored private let artworks = ArtworkCache()
    @ObservationIgnored lazy var enrichment = Enrichment(library: library)
    @ObservationIgnored lazy var downloads = DownloadQueue(library: library)
    @ObservationIgnored lazy var listening = Listening(library: library)
    @ObservationIgnored lazy var room = RoomCorrectionState(library: library)
    @ObservationIgnored private lazy var transfers = BackgroundHold(
        name: "Fetching songs from your devices",
        continued: BackgroundHold.transfers,
        expired: { [unowned self] in transferHoldLapsed = true }
    )
    @ObservationIgnored private var transferHoldLapsed = false

    var tidalAuthorization: TidalAuthorization?
    var tidalBusy = false
    var corrections: [AutoEqDevice] = []
    var correctionsFor: String?
    var searchingCorrections = false
    var applyingCorrection = false
    var scrobbleAccount: ScrobbleAccount?
    var scrobbleStatus: String?
    var providers = ProviderAccounts()
    var online: Bool
    var statistics: LibraryStatistics?
    var listeningDetail: ListeningDetail?
    private(set) var listenedCovers = ListenedCovers()
    var libraryBreakdown: LibraryBreakdown?
    var statisticsLoading = false
    private(set) var statisticsDays: UInt32?
    var lyrics: LyricSheet?
    var lyricsLoading = false
    var recognitionKey = ""
    var recognition: [RecognitionCandidate] = []
    var recognitionBusy = false
    var analysis: AnalysisReport?
    var analysisBusy = false
    var analysisProgress: Float?
    var inspector: Inspector?
    var sharing: SharedLink?
    @ObservationIgnored private var shareRequest = 0
    var inspectorTrack: UInt64?
    var inspectorArtist: ArtistLookup = .idle
    var playlists: [MusicPlaylist] = [] {
        didSet { foldedNames = Dictionary(playlists.map { ($0.id, $0.name.searchFolded) }, uniquingKeysWith: { kept, _ in kept }) }
    }
    @ObservationIgnored private var foldedNames: [UInt64: String] = [:]
    var playlistRows: [Track] = []
    var openPlaylist: UInt64?
    var playlistToShow: UInt64?
    var onlineQuery = "" {
        didSet { if onlineQuery != oldValue { askOnline() } }
    }
    var onlineFound = OnlineFound()
    var wanted: [WantedSong] = []
    var searching = false
    var enriching = false
    var samples: [Float] = []
    @ObservationIgnored private var artStamps: [UInt64: ArtStamp] = [:]
    @ObservationIgnored private var artStamped = 0
    private(set) var artEpoch = 0
    var portraitGeneration = 0
    var syncConfig: SyncConfig?
    @ObservationIgnored private var secretsUnread = false
    @ObservationIgnored private var unreadReported = false
    var syncStatus: SyncStatus?
    var syncHandoffs: [HandedOff] = []
    var syncFolder: String?
    var syncNotice: String?
    var playlistSync: PlaylistSync?
    var transferring = false
    var syncAskedAt: Date?
    var syncLanding = 0
    var pairingOffered: String?
    @ObservationIgnored private var joinWaiting: String?
    var tagsUndoable = false
    var organising = false
    var organiseUndo: (folder: String, back: [FiledSong])?

    @ObservationIgnored private var inspectorRequest = 0
    @ObservationIgnored private var inspectorArtistRequest = 0
    @ObservationIgnored private var statisticsRequest = 0
    @ObservationIgnored private var correctionRequest = 0
    @ObservationIgnored private var analysisRequest = 0
    @ObservationIgnored private var recognitionRequest = 0
    @ObservationIgnored private var lyricRequest = 0
    @ObservationIgnored private var playlistRequest = 0
    @ObservationIgnored private var searchRequest = 0
    @ObservationIgnored private var onlinePending: Task<Void, Never>?
    @ObservationIgnored private var onlineAnswers = OnlineAnswers()
    @ObservationIgnored private var submitting: Task<Void, Never>?
    @ObservationIgnored private var visualization: Task<Void, Never>?
    @ObservationIgnored private var syncPolling: Task<Void, Never>?
    @ObservationIgnored private var nearby: NearbyDevices?
    @ObservationIgnored private var network: NWPathMonitor?
    @ObservationIgnored private var power: (any NSObjectProtocol)?
    @ObservationIgnored private var arriving: Task<Void, Never>?
    @ObservationIgnored private var landing: Task<Void, Never>?
    @ObservationIgnored private var landedSongs = 0
    @ObservationIgnored private(set) var unmetered = false
    @ObservationIgnored private var artRevision: UInt64?
    @ObservationIgnored var foreground = true {
        didSet {
            library.nudgePoll()
            enrichment.foregroundChanged(foreground)
            downloads.foregroundChanged(foreground)
            tapHoldersChanged()
        }
    }
    @ObservationIgnored private var landingRefused: [UInt64: RefusedLanding] = [:]
    @ObservationIgnored private var leftAt: Date?

    init(library: Library) {
        self.library = library
        online = settings.object(forKey: "online") == nil ? true : settings.bool(forKey: "online")
        syncFolder = settings.string(forKey: "sync-folder")
    }

    var correctionBusy: Bool { searchingCorrections || applyingCorrection }

    private func work(
        _ action: @escaping @MainActor (MobilePlayer) async throws -> Void,
        finally: @escaping @MainActor () -> Void = {}
    ) {
        Task {
            defer { finally() }
            do {
                let native = try await library.player()
                try await action(native)
            } catch is CancellationError {
            } catch {
                library.report(error)
            }
        }
    }

    private func aside<Value: Sendable>(
        _ native: MobilePlayer,
        _ body: @escaping @Sendable (MobilePlayer) throws -> Value
    ) async throws -> Value {
        try await Work.aside { try body(native) }
    }

    func initialize(_ native: MobilePlayer) {
        room.restore(native)
        native.configureOnline(enabled: online)
        Task { [weak self] in
            await self?.readSecrets()
            self?.begin(native)
        }
    }

    private func begin(_ native: MobilePlayer) {
        downloads.recover(native)
        downloads.wantsChanged()
        if let left = settings.string(forKey: Self.landingJournalKey) {
            let folders = library.folders
            Task { [weak self] in
                _ = try? await Work.aside { folders.delete(left) }
                guard let self, settings.string(forKey: Self.landingJournalKey) == left else { return }
                settings.removeObject(forKey: Self.landingJournalKey)
            }
        }
        submitting?.cancel()
        submitting = Task { [weak self] in await self?.submitScrobbles(native) }
        watchNetwork()
        syncPolling?.cancel()
        syncPolling = Task { [weak self] in await self?.pollSync(native) }
        work { [weak self] native in try await self?.applySync(native) }
        Task { [weak self] in
            await self?.library.forgetMissing()
            await self?.refreshChangedArt(native)
            self?.enrichment.begin()
        }
    }

    private func readSecrets() async {
        let store = accounts
        guard let read = try? await Work.aside({
            SecretsRead(
                providers: Result { try store.readProviders() },
                scrobbling: Result { try store.readScrobbling() },
                recognition: Result { try store.readRecognitionKey() },
                sync: Result { try store.readSync() }
            )
        }) else { return }
        var failure: (any Swift.Error)?
        switch read.providers {
        case .success(let value): providers = value
        case .failure(let error): failure = error
        }
        switch read.scrobbling {
        case .success(let value): scrobbleAccount = value
        case .failure(let error): failure = error
        }
        switch read.recognition {
        case .success(let value): recognitionKey = value
        case .failure(let error): failure = error
        }
        switch read.sync {
        case .success(let value): syncConfig = value
        case .failure(let error): failure = error
        }
        secretsUnread = failure != nil
        if let failure, !unreadReported {
            unreadReported = true
            library.report(failure)
        }
        if let code = joinWaiting, syncConfig != nil { joinSync(code) }
    }

    private func submitScrobbles(_ native: MobilePlayer) async {
        var lastPlaying: UInt64?
        while !Task.isCancelled {
            try? await Task.sleep(for: Self.scrobbleEvery)
            guard let account = scrobbleAccount, online,
                !account.listenbrainzToken.isEmpty || !account.lastfmSession.isEmpty
            else { continue }
            let state = library.snapshot
            let playing = state?.transport == .playing ? state?.current?.id : nil
            let fresh = playing != lastPlaying ? playing : nil
            do {
                let count = try await aside(native) { try $0.submitScrobbles(account: account, playing: fresh) }
                lastPlaying = playing
                if count > 0 { scrobbleStatus = "Submitted \(counted(Int(count), "listen"))" }
            } catch {
                scrobbleStatus = explained(error)
            }
            do {
                let told = try await aside(native) { try $0.tellLoves(account: account) }
                if let said = Self.said(told) { scrobbleStatus = said }
            } catch {
                scrobbleStatus = explained(error)
            }
        }
    }

    private static func said(_ told: FavouritesTold) -> String? {
        let parts = [
            told.loved > 0 ? "loved \(counted(Int(told.loved), "favourite"))" : nil,
            told.takenBack > 0 ? "took back \(counted(Int(told.takenBack), "love"))" : nil,
            told.refused > 0 ? "\(counted(Int(told.refused), "love")) refused" : nil,
        ].compactMap { $0 }
        guard let first = parts.first else { return nil }
        let sentence = parts.joined(separator: ", ")
        return first.prefix(1).uppercased() + sentence.dropFirst()
    }

    func tick(_ native: MobilePlayer, _ snapshot: Snapshot) async {
        enrichment.playing(snapshot.current?.id)
        guard analysisBusy else { return }
        let progress = (try? await Work.aside { native.analysisProgress() }) ?? nil
        if analysisProgress != progress { analysisProgress = progress }
    }

    func setOnline(_ enabled: Bool) {
        work { [weak self] native in
            native.configureOnline(enabled: enabled)
            await MainActor.run {
                self?.online = enabled
                self?.settings.set(enabled, forKey: "online")
                self?.downloads.wantsChanged()
                if enabled {
                    self?.enrichment.begin()
                    self?.askOnline()
                } else {
                    self?.enrichment.read()
                }
            }
            try await self?.applySync(native)
        }
    }

    var syncRuns: Bool {
        guard let config = syncConfig, config.enabled else { return false }
        return online || config.nearbyOnly
    }

    func saveSync(_ config: SyncConfig) {
        let previous = syncConfig
        let store = accounts
        syncConfig = config
        work { [weak self] native in
            do {
                try await Work.inSecrets { try store.writeSync(config) }
            } catch {
                if self?.syncConfig == config { self?.syncConfig = previous }
                throw error
            }
            try await self?.applySync(native)
        }
    }

    func joinSync(_ code: String) {
        guard var config = syncConfig else {
            joinWaiting = code
            library.notice("Joining as soon as this iPhone's settings are read")
            return
        }
        joinWaiting = nil
        config.key = code.trimmingCharacters(in: .whitespacesAndNewlines)
        config.enabled = true
        saveSync(config)
    }

    func offerPairing(_ link: String) {
        guard let code = syncCodeInLink(link: link) else {
            library.notice("That link holds no pairing code")
            return
        }
        guard code != syncStatus?.code, code != syncConfig?.key else {
            library.notice("This iPhone already syncs with those devices")
            return
        }
        pairingOffered = code
    }

    func newSyncGroup() {
        guard var config = syncConfig else { return }
        do {
            config.key = try syncNewCode()
            saveSync(config)
        } catch {
            library.report(error)
        }
    }

    func forgetSyncDevices(_ devices: [SyncedDevice]) {
        let ids = devices.map(\.id)
        let names = devices.map(\.name)
        guard !ids.isEmpty else { return }
        work { [weak self] native in
            let status = try await Work.aside {
                for id in ids { _ = try native.syncForgetDevice(id: id) }
                return native.syncStatus()
            }
            guard let self else { return }
            if syncStatus != status { syncStatus = status }
            library.notice(names.count == 1 ? "Removed \(names[0])" : "Removed \(counted(names.count, "device"))")
        }
    }

    func chooseSyncFolder(_ folder: MusicFolder) {
        syncFolder = folder.id
        settings.set(folder.id, forKey: "sync-folder")
    }

    func loadPlaylistSync(_ id: UInt64) {
        work { [weak self] native in
            let found = try await Work.aside {
                PlaylistSync(playlist: id, missing: try native.syncMissingFrom(playlist: id), kept: try native.syncKeepsSongs(playlist: id))
            }
            await MainActor.run { self?.playlistSync = found }
        }
    }

    func fetchMissing(_ id: UInt64, from device: SyncedDevice) {
        let known = device.id
        let name = device.name
        work { [weak self] native in
            let asked = try await Work.aside { try native.syncFetchMissing(playlist: id, device: known) }
            self?.library.notice(
                asked > 0 ? "Asking \(name) for \(counted(Int(asked), "song"))" : "\(name) has nothing this playlist lacks"
            )
        }
    }

    func keepSongs(_ id: UInt64, _ keep: Bool) {
        work { [weak self] native in
            let found = try await Work.aside {
                try native.syncKeepSongs(playlist: id, keep: keep)
                return PlaylistSync(playlist: id, missing: try native.syncMissingFrom(playlist: id), kept: try native.syncKeepsSongs(playlist: id))
            }
            await MainActor.run { self?.playlistSync = found }
        }
    }

    func continueFrom(_ handoff: HandedOff) {
        let known = handoff.id
        let device = handoff.device
        work { [weak self] native in
            let loaded = try await Work.inSync { try native.syncContinueFrom(device: known) }
            await MainActor.run { [weak self] in
                guard let self else { return }
                library.notice("Continuing \(counted(Int(loaded), "song")) from \(device)")
                if handoff.playing {
                    library.nowPlaying.activate()
                    library.request { try $0.play() }
                }
            }
        }
    }

    var syncActivity: SyncActivity? {
        guard let status = syncStatus, status.standing != .off else { return nil }
        let active = status.transfers.filter { $0.state == .waiting || $0.state == .moving }
        if !active.isEmpty || syncLanding > 0 {
            let sized = active.filter { $0.size > 0 }
            let size = sized.reduce(UInt64(0)) { $0 + $1.size }
            let received = sized.reduce(UInt64(0)) { $0 + min($1.received, $1.size) }
            let fraction = size == 0 || sized.count != active.count ? nil : Double(received) / Double(size)
            return .fetching(songs: active.count + syncLanding, fraction: fraction)
        }
        return syncAskedAt == nil ? nil : .syncing
    }

    private static func fetched(_ status: SyncStatus) -> ContinuedProgress {
        let active = status.transfers.filter { $0.state == .waiting || $0.state == .moving }
        let size = active.reduce(UInt64(0)) { $0 + $1.size }
        let received = active.reduce(UInt64(0)) { $0 + min($1.received, $1.size) }
        return ContinuedProgress(
            done: Int64(clamping: received),
            total: Int64(clamping: size),
            subtitle: counted(active.count, "song") + " left"
        )
    }

    var syncUpToDate: Bool {
        guard let status = syncStatus, status.standing != .off else { return false }
        return syncActivity == nil && status.settled
    }

    func forceSync() {
        nearby?.refresh()
        syncAskedAt = Date()
        work { [weak self] native in
            await self?.library.forgetMissing()
            try await Work.aside { native.syncForce() }
        }
    }

    func stopForErasing() async {
        let polling = syncPolling
        let arrivals = arriving
        let landed = landing
        syncPolling = nil
        arriving = nil
        landing = nil
        polling?.cancel()
        arrivals?.cancel()
        landed?.cancel()
        submitting?.cancel()
        visualization?.cancel()
        onlinePending?.cancel()
        await polling?.value
        await arrivals?.value
        await landed?.value
        nearby?.follow(record: nil)
        syncAskedAt = nil
        listening.stop()
        await enrichment.stopForErasing()
        await downloads.stopForErasing()
    }

    func wentToBackground() {
        leftAt = Date()
    }

    func cameForward() {
        let wasAway = leftAt != nil
        leftAt = nil
        if secretsUnread {
            Task { [weak self] in
                guard let self else { return }
                await readSecrets()
                if syncConfig != nil { work { [weak self] native in try await self?.applySync(native) } }
            }
        }
        guard wasAway else { return }
        library.openAgain()
        library.rescanUnasked(.everything)
        guard syncRuns else {
            Task { [weak self] in await self?.library.forgetMissing() }
            return
        }
        if !transferring || transferHoldLapsed {
            transferHoldLapsed = false
            nearby?.restart()
            work { [weak self] native in
                var failure: (any Swift.Error)?
                do {
                    try await self?.applySync(native)
                } catch {
                    failure = error
                }
                if let changed = await self?.library.forgetMissing(), changed.hidden > 0, failure == nil {
                    try await Work.aside { native.syncForce() }
                }
                if let failure { throw failure }
            }
            return
        }
        work { [weak self] native in
            guard let changed = await self?.library.forgetMissing(), changed.hidden > 0 else { return }
            try await Work.aside { native.syncForce() }
        }
    }

    private func answerSyncAsked(_ status: SyncStatus) {
        guard let asked = syncAskedAt else { return }
        let waited = Date().timeIntervalSince(asked)
        guard waited >= Self.syncAnswersAfter else { return }
        let others = status.devices.filter { !$0.thisDevice && ($0.online || $0.nearby) }
        if status.standing == .off {
            syncAskedAt = nil
        } else if syncActivity == .syncing, status.settled {
            syncAskedAt = nil
            library.notice(others.isEmpty ? "Synced · no other device is online" : "Synced with \(counted(others.count, "device"))")
        } else if status.standing == .refused || (status.standing == .unreachable && others.isEmpty) {
            syncAskedAt = nil
            library.notice(syncStanding(status, config: syncConfig, online: online))
        } else if waited >= Self.syncGivesUpAfter {
            syncAskedAt = nil
        }
    }

    private func applySync(_ native: MobilePlayer) async throws {
        guard let config = syncConfig else { return }
        let enabled = syncRuns
        do {
            let status = try await Work.inSync {
                guard enabled else {
                    native.syncStop()
                    return native.syncStatus()
                }
                return try native.syncStart(config: config)
            }
            let free = unmetered
            try await Work.inSync { native.syncNetwork(unmetered: free) }
            if !status.code.isEmpty, var minted = syncConfig, minted.key == config.key, minted.key != status.code {
                minted.key = status.code
                let written = minted
                let store = accounts
                try await Work.inSecrets { try store.writeSync(written) }
                if var latest = syncConfig, latest.key == config.key {
                    latest.key = status.code
                    syncConfig = latest
                }
            }
            syncStatus = status
            await announceNearby(native)
        } catch {
            syncStatus = try? await Work.aside { native.syncStatus() }
            throw error
        }
    }

    private func announceNearby(_ native: MobilePlayer) async {
        guard let record = try? await Work.aside({ native.syncNearbyRecord() }) else {
            nearby?.follow(record: nil)
            return
        }
        if nearby == nil { nearby = NearbyDevices(native: native) }
        nearby?.follow(record: record)
    }

    private func watchNetwork() {
        guard network == nil else { return }
        let monitor = NWPathMonitor()
        monitor.pathUpdateHandler = { @Sendable [weak self] path in
            let free = path.status == .satisfied && !path.isExpensive && !path.isConstrained
            Task { @MainActor in self?.networkChanged(free) }
        }
        monitor.start(queue: DispatchQueue(label: "org.resonate.network"))
        network = monitor
        power = NotificationCenter.default.addObserver(
            forName: .NSProcessInfoPowerStateDidChange,
            object: nil,
            queue: .main
        ) { @Sendable [weak self] _ in
            Task { @MainActor in self?.downloads.wantsChanged() }
        }
    }

    private func networkChanged(_ free: Bool) {
        guard free != unmetered else { return }
        unmetered = free
        work { [weak self] native in
            try await Work.inSync { native.syncNetwork(unmetered: free) }
            await self?.announceNearby(native)
        }
        enrichment.networkChanged(free: free)
        if free { downloads.wantsChanged() }
    }

    private func pollSync(_ native: MobilePlayer) async {
        var arrivals: UInt64 = 0
        var failing = false
        while !Task.isCancelled {
            try? await Task.sleep(for: syncAskedAt != nil ? Self.syncAsked : foreground || transferring ? Self.syncForeground : Self.syncBackground)
            do {
                let (status, waiting, handoffs, record) = try await Work.aside {
                    let status = native.syncStatus()
                    let waiting = try native.syncReceived()
                    let handoffs = status.standing == .off ? [] : try native.syncHandoffs()
                    return (status, waiting, handoffs, native.syncNearbyRecord())
                }
                if nearby == nil { nearby = NearbyDevices(native: native) }
                nearby?.follow(record: record)
                if syncStatus != status { syncStatus = status }
                let moving = status.moving > 0 || !landable(waiting).isEmpty
                if transferring != moving { transferring = moving }
                transfers.hold(transferring)
                if transferring { transfers.progress(Self.fetched(status)) }
                answerSyncAsked(status)
                if !waiting.isEmpty, landing == nil {
                    landing = Task { [weak self] in
                        guard let self else { return }
                        landedSongs += await landReceived(native, waiting)
                        landing = nil
                    }
                }
                if landedSongs > 0, !transferring, landing == nil {
                    library.notice("Kept \(counted(landedSongs, "song")) from your devices")
                    landedSongs = 0
                }
                if syncHandoffs != handoffs { syncHandoffs = handoffs }
                if arrivals != status.arrivals {
                    arrivals = status.arrivals
                    arriving?.cancel()
                    arriving = Task { [weak self] in await self?.adoptArrivals(native) }
                }
                failing = false
            } catch {
                if !failing { library.report(error) }
                failing = true
            }
        }
    }

    private func adoptArrivals(_ native: MobilePlayer) async {
        await library.refresh()
        guard !Task.isCancelled else { return }
        do {
            let found = try await Work.aside { try native.playlists() }
            if playlists != found { playlists = found }
            if let open = openPlaylist, playlists.contains(where: { $0.id == open }) {
                await reloadPlaylist(native, open)
            }
        } catch {
            library.report(error)
        }
    }

    private func landable(_ waiting: [ReceivedSong]) -> [ReceivedSong] {
        guard writableFolder(syncFolder) != nil else { return [] }
        let now = Date()
        return waiting.filter { (landingRefused[$0.id]?.until ?? .distantPast) <= now }
    }

    private func writableFolder(_ preferred: String?) -> MusicFolder? {
        let destinations = library.folderList.filter { $0.readable && $0.writable }
        return destinations.first { $0.id == preferred } ?? destinations.first
    }

    private func landReceived(_ native: MobilePlayer, _ waiting: [ReceivedSong]) async -> Int {
        guard let folder = writableFolder(syncFolder) else {
            syncNotice = "Add a music folder with write access to keep songs from your devices"
            return 0
        }
        let folders = library.folders
        let still = Set(waiting.map(\.id))
        landingRefused = landingRefused.filter { still.contains($0.key) }
        let due = landable(waiting)
        var landed = 0
        syncLanding = due.count
        defer { syncLanding = 0 }
        for received in due {
            guard !Task.isCancelled else { return landed }
            let name = received.name
            var created: String?
            do {
                let (uri, descriptor) = try await Work.aside { try folders.fresh(in: folder.id, named: name) }
                created = uri
                settings.set(uri, forKey: Self.landingJournalKey)
                _ = try await Work.aside { try native.syncReceive(id: received.id, descriptor: descriptor, uri: uri) }
                created = nil
                folders.written(uri)
                settings.removeObject(forKey: Self.landingJournalKey)
                landed += 1
                landingRefused[received.id] = nil
            } catch {
                if let created {
                    _ = try? await Work.aside { folders.delete(created) }
                    settings.removeObject(forKey: Self.landingJournalKey)
                }
                await refuseLanding(native, received.id)
                let explanation = explained(error)
                if explanation != syncNotice { library.report(error) }
                syncNotice = explanation
            }
            syncLanding = max(syncLanding - 1, 0)
        }
        if landed > 0 {
            syncNotice = nil
            await library.refresh()
            await refreshWanted(native)
            await refreshChangedArt(native)
            enrichment.begin()
        }
        return landed
    }

    private func refuseLanding(_ native: MobilePlayer, _ id: UInt64) async {
        let tries = (landingRefused[id]?.tries ?? 0) + 1
        guard tries >= Self.landingTriesAtMost else {
            landingRefused[id] = RefusedLanding(until: Date().addingTimeInterval(Self.landingPause), tries: tries)
            return
        }
        landingRefused[id] = nil
        _ = try? await Work.aside { try native.syncRefuseReceived(id: id) }
    }

    func foldedName(of playlist: MusicPlaylist) -> String {
        foldedNames[playlist.id] ?? playlist.name.searchFolded
    }

    func artGeneration(of id: UInt64) -> Int {
        artEpoch * Self.artGenerationsAtMost + stamp(of: id).value
    }

    private func stamp(of id: UInt64) -> ArtStamp {
        if let kept = artStamps[id] { return kept }
        let made = ArtStamp()
        artStamps[id] = made
        return made
    }

    func cachedArtwork(_ track: Track, bucket: ArtworkBucket) -> UIImage? {
        artworks.cached(artworkRequest(track, bucket: bucket))
    }

    func cachedPortrait(_ mbid: String, bucket: ArtworkBucket) -> UIImage? {
        artworks.cached(portraitRequest(mbid, bucket: bucket))
    }

    private func artworkRequest(_ track: Track, bucket: ArtworkBucket) -> ArtworkRequest {
        ArtworkRequest(subject: .track(track.id), revision: track.artRevision, generation: artGeneration(of: track.id), bucket: bucket)
    }

    private func portraitRequest(_ mbid: String, bucket: ArtworkBucket) -> ArtworkRequest {
        ArtworkRequest(subject: .portrait(mbid), revision: 0, generation: portraitGeneration, bucket: bucket)
    }

    func artwork(_ track: Track, bucket: ArtworkBucket) async -> UIImage? {
        let id = track.id
        let request = artworkRequest(track, bucket: bucket)
        guard let native = try? await library.player() else { return nil }
        return await artworks.image(request, identifying: { try native.artworkPicture(id: id) }, reading: { try native.picture(picture: $0) })
    }

    func portrait(_ mbid: String, bucket: ArtworkBucket) async -> UIImage? {
        let request = portraitRequest(mbid, bucket: bucket)
        guard let native = try? await library.player() else { return nil }
        return await artworks.image(request) { try native.artistPortrait(mbid: mbid) }
    }

    func portraitsChanged() {
        portraitGeneration += 1
    }

    func scanned() {
        Task { await refreshChangedArt() }
        enrichment.begin()
    }

    func refreshChangedArt() async {
        guard let native = try? await library.player() else { return }
        await refreshChangedArt(native)
    }

    func refreshChangedArt(_ native: MobilePlayer) async {
        let asked = artRevision
        let since = asked ?? 0
        guard let changed = try? await Work.aside({ try native.artChangedSince(revision: since) }) else { return }
        let known = asked != nil
        artRevision = max(artRevision ?? 0, changed.revision)
        guard known, !changed.tracks.isEmpty else { return }

        artStamped += changed.tracks.count
        guard artStamped <= Self.artGenerationsAtMost, artStamps.count <= Self.artStampsAtMost else {
            artStamped = 0
            artStamps = [:]
            artEpoch += 1
            return
        }
        for id in changed.tracks { stamp(of: id).value += 1 }
    }

    func loadPlaylists() {
        work { [weak self] native in
            let found = try await Work.aside { try native.playlists() }
            await MainActor.run {
                if self?.playlists != found { self?.playlists = found }
            }
        }
    }

    func selectPlaylist(_ id: UInt64?) {
        if openPlaylist != id { playlistRows = [] }
        openPlaylist = id
        if let id { work { [weak self] native in await self?.reloadPlaylist(native, id) } }
    }

    func leavePlaylist(_ id: UInt64) {
        guard openPlaylist == id else { return }
        openPlaylist = nil
        playlistRows = []
    }

    func createPlaylist(_ name: String, with tracks: [Track] = [], then: PlaylistEdited? = nil) {
        let ids = tracks.map(\.id)
        editPlaylist({ native in
            let id = try native.createPlaylist(name: name)
            if !ids.isEmpty { try native.addToPlaylist(id: id, tracks: ids) }
        }, then: then)
    }

    func addToPlaylist(_ id: UInt64, _ tracks: [Track], then: PlaylistEdited? = nil) {
        let ids = tracks.map(\.id)
        editPlaylist({ native in try native.addToPlaylist(id: id, tracks: ids) }, then: then)
    }

    func editPlaylist(_ action: @escaping @Sendable (MobilePlayer) throws -> Void, then: PlaylistEdited? = nil) {
        work { [weak self] native in
            do {
                let found = try await Work.inLibrary {
                    try action(native)
                    return try native.playlists()
                }
                self?.adoptPlaylists(found, native)
                then?(nil)
            } catch {
                await self?.reloadOpenPlaylist()
                guard let then else { throw error }
                then(explained(error))
            }
        }
    }

    func reloadOpenPlaylist() async {
        guard let open = openPlaylist, let native = try? await library.player() else { return }
        await reloadPlaylist(native, open)
    }

    func undoPlaylistEdit() {
        work { [weak self] native in
            let (undone, found) = try await Work.inLibrary { (try native.undoPlaylist(), try native.playlists()) }
            guard let self else { return }
            adoptPlaylists(found, native)
            library.notice(undone ? "Undid the last playlist edit" : "There is no playlist edit to undo")
        }
    }

    private func adoptPlaylists(_ found: [MusicPlaylist], _ native: MobilePlayer) {
        if playlists != found { playlists = found }
        guard let open = openPlaylist else { return }
        if found.contains(where: { $0.id == open }) {
            Task { await reloadPlaylist(native, open) }
        } else {
            selectPlaylist(nil)
        }
    }

    private func reloadPlaylist(_ native: MobilePlayer, _ id: UInt64) async {
        playlistRequest += 1
        let request = playlistRequest
        do {
            let (rows, found) = try await Work.aside {
                let rows = try Library.everyPage { try native.playlistTracks(id: id, offset: $0, limit: Library.page) }
                return (rows, try native.playlists())
            }
            guard request == playlistRequest else { return }
            if openPlaylist == id, playlistRows != rows { playlistRows = rows }
            if playlists != found { playlists = found }
        } catch {
            library.report(error)
        }
    }

    static let rankedShown: UInt32 = 25

    func loadStatistics(days: UInt32 = 0) {
        statisticsRequest += 1
        let request = statisticsRequest
        statisticsLoading = true
        work({ [weak self] native in
            let zone = TimeZone.current
            let identifier = zone.identifier
            let offset = Int32(zone.secondsFromGMT())
            let shown = Features.rankedShown
            let found = try await Work.aside { try native.statistics(days: days) }
            let detail = try await Work.aside {
                try native.listeningDetail(
                    days: days, limit: shown, timeZone: identifier, utcOffsetSeconds: offset)
            }
            let covers = try await Work.aside { ListenedCovers.of(detail, from: native) }
            let breakdown = try await Work.aside { try native.libraryBreakdown() }
            guard let self, request == statisticsRequest else { return }
            if statistics != found { statistics = found }
            if listeningDetail != detail { listeningDetail = detail }
            if listenedCovers != covers { listenedCovers = covers }
            if libraryBreakdown != breakdown { libraryBreakdown = breakdown }
            statisticsDays = days
        }, finally: { [weak self] in
            if request == self?.statisticsRequest { self?.statisticsLoading = false }
        })
    }

    func loadInspector(_ id: UInt64) {
        inspectorRequest += 1
        let request = inspectorRequest
        if inspectorTrack != id {
            inspectorTrack = id
            inspector = nil
            inspectorArtistRequest += 1
            inspectorArtist = .idle
        }
        work { [weak self] native in
            let found = try await Work.aside { try native.inspector(id: id) }
            if request == self?.inspectorRequest { self?.inspector = found }
        }
    }

    func loadArtist(_ id: UInt64) {
        inspectorArtistRequest += 1
        let request = inspectorArtistRequest
        inspectorArtist = .looking
        work({ [weak self] native in
            let found = try await Work.aside { try native.artistInformation(id: id) }
            guard let self, request == inspectorArtistRequest else { return }
            inspectorArtist = found.map(ArtistLookup.found) ?? .missing
        }, finally: { [weak self] in
            guard let self, request == inspectorArtistRequest, case .looking = inspectorArtist else { return }
            inspectorArtist = .idle
        })
    }

    func loadLyrics(_ id: UInt64, refresh: Bool = false) {
        lyricRequest += 1
        let request = lyricRequest
        if lyrics?.trackId != id { lyrics = nil }
        lyricsLoading = true
        work { [weak self] native in
            defer { if request == self?.lyricRequest { self?.lyricsLoading = false } }
            let sheet = try await Work.aside { try native.lyrics(id: id, refresh: refresh) }
            await MainActor.run { if request == self?.lyricRequest { self?.lyrics = sheet } }
        }
    }

    func keepLyricText(_ id: UInt64, _ text: String) {
        work { [weak self] native in
            try await Work.aside { try native.keepLyricText(id: id, text: text) }
            self?.loadLyrics(id)
        }
    }

    func songTags(_ ids: [UInt64]) async throws -> [SongTags] {
        let native = try await library.player()
        return try await Work.aside { try ids.map { try native.songTags(id: $0) } }
    }

    func tagsWritable(_ track: Track) -> Bool {
        guard library.native?.tagsWritable(name: track.name) == true,
            let root = Folders.root(of: track.uri)
        else { return false }
        return library.folderList.first { $0.id == root }?.writable ?? false
    }

    func editTags(_ tracks: [Track], _ change: SongTags) {
        let uris = tracks.map(\.uri)
        let folders = library.folders
        work { [weak self] native in
            let told = try await Work.aside {
                try folders.withPaths(uris) { try native.editTags(songs: $0, change: change) }
            }
            await self?.tagged(native, told, undoing: false)
        }
    }

    func share(_ track: Track) {
        shareRequest += 1
        let request = shareRequest
        let id = track.id
        let title = track.shownTitle
        work { [weak self] native in
            let found = try await Work.aside { try native.shareLink(id: id) }
            guard request == self?.shareRequest else { return }
            guard let link = found.flatMap({ URL(string: $0) }) else {
                self?.library.notice("There is no link to share for “\(title)”")
                return
            }
            self?.sharing = SharedLink(title: title, url: link)
        }
    }

    func undoTags() {
        let folders = library.folders
        work { [weak self] native in
            let told = try await Work.aside {
                try folders.withPaths(native.tagsToUndo()) { try native.undoTags(songs: $0) }
            }
            await self?.tagged(native, told, undoing: true)
        }
    }

    private func tagged(_ native: MobilePlayer, _ told: TagsWritten, undoing: Bool) async {
        tagsUndoable = !native.tagsToUndo().isEmpty
        if told.written > 0 { await library.refresh() }
        if let failure = told.failure {
            library.report(TagFailure(text: told.written > 0 ? "\(failure) (\(counted(Int(told.failed), "song")) left as they were)" : failure))
        } else if told.written > 0 {
            library.notice(undoing ? "Put back the tags of \(counted(Int(told.written), "song"))" : "Saved the tags of \(counted(Int(told.written), "song"))")
        } else {
            library.notice("Nothing to change")
        }
    }

    func organisePlan(_ folder: MusicFolder, layout: String) async throws -> OrganisePlan {
        let native = try await library.player()
        let prefix = library.folders.prefix(of: folder.id)
        return try await Work.aside { try native.planOrganise(folder: prefix, layout: layout) }
    }

    func organise(_ folder: MusicFolder, _ moves: [FiledSong]) {
        file(folder.id, moves, undoing: false)
    }

    func putFilesBack() {
        guard let (folder, back) = organiseUndo else { return }
        file(folder, back, undoing: true)
    }

    private func file(_ folder: String, _ moves: [FiledSong], undoing: Bool) {
        let folders = library.folders
        organising = true
        if library.scanning { library.notice("Filing songs once the scan ends") }
        library.afterScans { [weak self] in
            guard let self else { return }
            defer { organising = false }
            do {
                let native = try await library.player()
                let filed = try await Work.aside { try folders.file(folder, moves) }
                try await Work.inLibrary { try native.documentsMoved(moved: filed.moved) }
                organiseUndo = undoing || filed.back.isEmpty ? nil : (folder, filed.back)
                await library.refresh()
                let moved = counted(filed.moved.count, "song")
                let said = undoing ? "Put \(moved) back where they were" : "Filed \(moved)"
                library.notice(filed.failed > 0 ? "\(said); \(counted(filed.failed, "song")) could not be moved" : said)
            } catch is CancellationError {
            } catch {
                library.report(error)
            }
        }
    }

    func saveRecognitionKey(_ value: String) {
        let store = accounts
        Task {
            do {
                try await Work.inSecrets { try store.writeRecognitionKey(value) }
                recognitionKey = value
            } catch {
                library.report(error)
            }
        }
    }

    func recognise(_ id: UInt64) {
        recognitionRequest += 1
        let request = recognitionRequest
        recognition = []
        recognitionBusy = true
        let key = recognitionKey
        work({ [weak self] native in
            let found = try await Work.aside { try native.recogniseTrack(id: id, acoustidKey: key) }
            guard let self, request == recognitionRequest else { return }
            recognition = found
            if found.isEmpty { library.notice("No recording could be identified from this audio") }
        }, finally: { [weak self] in
            if request == self?.recognitionRequest { self?.recognitionBusy = false }
        })
    }

    func analyse(_ id: UInt64) {
        recognitionRequest += 1
        recognition = []
        recognitionBusy = false
        analysisRequest += 1
        let request = analysisRequest
        analysis = nil
        analysisProgress = nil
        analysisBusy = true
        work({ [weak self] native in
            do {
                let report = try await Work.aside { try native.analyseTrack(id: id) }
                if request == self?.analysisRequest { self?.analysis = report }
            } catch {
                if request == self?.analysisRequest { throw error }
            }
        }, finally: { [weak self] in
            if request == self?.analysisRequest { self?.analysisBusy = false }
        })
    }

    func cancelAnalysis() {
        guard analysisBusy else { return }
        analysisRequest += 1
        let request = analysisRequest
        analysisBusy = false
        analysisProgress = nil
        work { [weak self] native in
            if request == self?.analysisRequest { native.cancelAnalysis() }
        }
    }

    func enrich(_ track: Track) {
        guard !enriching else { return }
        enriching = true
        let id = track.id
        work({ [weak self] native in
            _ = try await Work.aside { try native.enrichTrack(id: id) }
            await self?.library.refresh()
            await self?.refreshChangedArt(native)
        }, finally: { [weak self] in
            self?.enriching = false
        })
    }

    func searchCorrections(_ text: String) {
        correctionRequest += 1
        let request = correctionRequest
        searchingCorrections = true
        work({ [weak self] native in
            let found = try await Work.aside { try native.searchAutoEq(text: text) }
            guard let self, request == correctionRequest else { return }
            corrections = found
            correctionsFor = text
        }, finally: { [weak self] in
            if request == self?.correctionRequest { self?.searchingCorrections = false }
        })
    }

    func clearCorrections() {
        correctionRequest += 1
        corrections = []
        correctionsFor = nil
        searchingCorrections = false
    }

    func applyCorrection(_ device: AutoEqDevice) {
        guard !applyingCorrection else { return }
        applyingCorrection = true
        let id = device.id
        work({ [weak self] native in
            let found = try await Work.aside { try native.autoEqProfile(id: id) }
            guard let self else { return }
            guard let found else {
                library.notice("No correction could be read for \(device.name)")
                return
            }
            load(found)
        }, finally: { [weak self] in
            self?.applyingCorrection = false
        })
    }

    func applyProfile(name: String, bytes: Data) {
        guard !applyingCorrection else { return }
        applyingCorrection = true
        work({ [weak self] _ in
            let found = try await Work.aside { try parseEqProfile(bytes: bytes, name: name) }
            self?.load(found)
        }, finally: { [weak self] in
            self?.applyingCorrection = false
        })
    }

    private func load(_ found: EqualiserImport) {
        let curve = found.curve
        library.updatePreferences { current in
            var next = current
            next.equaliserEnabled = true
            next.equaliser = curve
            return next
        }
        library.notice(Self.loaded(found))
    }

    private static func loaded(_ found: EqualiserImport) -> String {
        let name = found.curve.name.isEmpty ? "Profile" : found.curve.name
        var said = ["Loaded \(name): \(counted(found.curve.bands.count, "band"))"]
        if found.converted { said.append("fitted from a graphic curve") }
        if found.passedOver > 0 { said.append("\(counted(Int(found.passedOver), "line")) passed over") }
        if found.approximated > 0 { said.append("\(counted(Int(found.approximated), "filter")) approximated") }
        if found.clamped > 0 { said.append("\(counted(Int(found.clamped), "value")) held to the limits") }
        return said.joined(separator: " · ")
    }

    func saveScrobbling(_ account: ScrobbleAccount, includeHistory: Bool) {
        let previous = scrobbleAccount
        let store = accounts
        Task {
            do {
                try await Work.inSecrets { try store.writeScrobbling(account) }
            } catch {
                library.report(error)
                return
            }
            scrobblingSaved(account, replacing: previous, includeHistory: includeHistory)
        }
    }

    private func scrobblingSaved(_ account: ScrobbleAccount, replacing previous: ScrobbleAccount?, includeHistory: Bool) {
        scrobbleAccount = account
        scrobbleStatus = "Scrobbling accounts saved"

        let listenbrainzStarts = !account.listenbrainzToken.isEmpty
            && (previous?.listenbrainzToken != account.listenbrainzToken || includeHistory)
        let lastfmStarts = !account.lastfmSession.isEmpty
            && (previous?.lastfmSession != account.lastfmSession || includeHistory)
        guard listenbrainzStarts || lastfmStarts else { return }

        work { [weak self] native in
            do {
                try await Work.aside {
                    if listenbrainzStarts { try native.startScrobbling(service: .listenBrainz, includeHistory: includeHistory) }
                    if lastfmStarts { try native.startScrobbling(service: .lastFm, includeHistory: includeHistory) }
                }
            } catch {
                self?.scrobbleStatus = "Accounts saved, but scrobbling could not start: \(explained(error))"
                throw error
            }
        }
    }

    func validateScrobbling(_ account: ScrobbleAccount) {
        work { [weak self] native in
            let found = try await Work.aside { try native.validateScrobbling(account: account) }
            await MainActor.run { self?.scrobbleStatus = found.joined(separator: " · ") }
        }
    }

    func signInLastFm(_ account: ScrobbleAccount, user: String, password: String) {
        work { [weak self] native in
            let session = try await Work.aside {
                try native.lastfmSignIn(key: account.lastfmKey, secret: account.lastfmSecret, user: user, password: password)
            }
            var signedIn = account
            signedIn.lastfmSession = session
            let store = self?.accounts
            let kept = signedIn
            try await Work.inSecrets { try store?.writeScrobbling(kept) }
            self?.scrobbleAccount = signedIn
            self?.scrobbleStatus = "Signed in to Last.fm"
            try await Work.aside { try native.startScrobbling(service: .lastFm, includeHistory: false) }
        }
    }

    func authorizeTidal(_ account: DownloadAccount) {
        guard !tidalBusy else { return }
        tidalBusy = true
        work({ [weak self] native in
            let authorization = try await Work.aside {
                try native.authorizeTidal(clientId: account.clientId, secret: account.clientSecret)
            }
            self?.tidalAuthorization = authorization
            guard let token = try await Work.aside({ try native.finishTidalAuthorization() }) else { return }

            var updated = account
            updated.refreshToken = token

            guard let self else { return }
            try await keepProviderAccount(updated)
            library.notice("Signed in to TIDAL")
        }, finally: { [weak self] in
            self?.tidalBusy = false
            self?.tidalAuthorization = nil
        })
    }

    func saveProviderAccount(_ account: DownloadAccount) {
        Task {
            do {
                try await keepProviderAccount(account)
                library.notice("\(account.provider.title) saved")
            } catch {
                library.report(error)
            }
        }
    }

    func turnProvider(_ provider: MusicProvider, on: Bool) {
        providers.turn(provider, on: on)
        accounts.writeSwitchedOff(providers.switchedOff)
        downloads.wantsChanged()
    }

    var downloadAccounts: [DownloadAccount] { providers.asked }

    func keepRenewedTidalToken(_ token: String) {
        var renewed = providers.account(.tidal)
        guard renewed.refreshToken != token else { return }
        renewed.refreshToken = token
        Task {
            do {
                try await keepProviderAccount(renewed)
            } catch {
                library.report(error)
            }
        }
    }

    private func keepProviderAccount(_ account: DownloadAccount) async throws {
        let before = providers
        providers.keep(account)
        let accounts = providers.accounts
        let store = self.accounts
        downloads.wantsChanged()
        do {
            try await Work.inSecrets { try store.writeProviderAccounts(accounts) }
        } catch {
            if providers.account(account.provider) == account {
                providers.keep(before.account(account.provider))
                downloads.wantsChanged()
            }
            throw error
        }
    }

    func saveWanted(_ songs: [SearchSong]) {
        work { [weak self] native in
            let outcome = try await Work.aside { () -> WantedSaving in
                var saved: UInt64 = 0
                for start in stride(from: 0, to: songs.count, by: Features.wantedChunk) {
                    do {
                        saved += try native.saveWanted(songs: Array(songs[start..<min(start + Features.wantedChunk, songs.count)]))
                    } catch {
                        return WantedSaving(saved: saved, failure: error)
                    }
                }
                return WantedSaving(saved: saved, failure: nil)
            }
            let (count, failure) = (outcome.saved, outcome.failure)
            await self?.refreshWanted(native)
            if let failure {
                self?.library.report(failure, after: "Saved \(count) of \(songs.count) recordings for later")
            } else {
                self?.library.notice("Saved \(count) recordings for later")
            }
        }
    }

    func loadWanted() {
        work { [weak self] native in await self?.refreshWanted(native) }
    }

    func rereadWanted() async {
        guard let native = try? await library.player() else { return }
        await refreshWanted(native)
    }

    func removeWanted(_ id: UInt64) {
        work { [weak self] native in
            try await Work.aside { try native.removeWanted(id: id) }
            await self?.refreshWanted(native)
        }
    }

    private func refreshWanted(_ native: MobilePlayer) async {
        do {
            wanted = try await Work.aside {
                var rows: [WantedSong] = []
                while true {
                    let page = try native.wantedSongs(offset: UInt32(rows.count), limit: Library.page)
                    rows += page
                    if page.count < Library.page { return rows }
                }
            }
            downloads.wantsChanged()
        } catch {
            library.report(error)
        }
    }

    func importPlaylist(_ name: String, _ songs: [SearchSong]) {
        work { [weak self] native in
            let (imported, found) = try await Work.inLibrary {
                (try native.importMatchedPlaylist(name: name, songs: songs), try native.playlists())
            }
            await MainActor.run {
                self?.playlists = found
                self?.library.notice("Imported \(imported.matched) local songs; \(imported.missing) still missing")
            }
        }
    }

    func onlineAnswer<Value: Sendable>(_ ask: @escaping @Sendable (MobilePlayer) throws -> Value) async -> Value? {
        do {
            let native = try await library.player()
            return try await Work.aside { try ask(native) }
        } catch {
            library.report(error)
            return nil
        }
    }

    private func askOnline() {
        searchRequest += 1
        onlinePending?.cancel()
        let request = searchRequest
        let text = onlineQuery.trimmingCharacters(in: .whitespacesAndNewlines)
        let chain = OnlineAsked.chain(text)

        guard online, !chain.isEmpty else {
            searching = false
            return
        }
        let kept = chain.compactMap { onlineAnswers.answer(for: $0) }
        if kept.count == chain.count {
            onlineFound = OnlineFound(text: text, answers: kept)
            searching = false
            return
        }

        searching = true
        onlinePending = Task { [weak self] in
            do { try await Task.sleep(for: OnlineAsked.settle) } catch { return }
            guard let self else { return }
            defer { if request == searchRequest { searching = false } }

            do {
                let native = try await library.player()
                for asked in chain {
                    guard request == searchRequest else { return }
                    let landed = try await answer(asked, by: native)
                    guard request == searchRequest else { return }
                    onlineFound = onlineFound.landing(landed, for: text)
                }
            } catch {
                guard request == searchRequest else { return }
                library.report(error)
            }
        }
    }

    private func answer(_ asked: OnlineAsked, by native: MobilePlayer) async throws -> OnlineAnswer {
        if let kept = onlineAnswers.answer(for: asked) { return kept }

        let fresh = try await Work.aside { try OnlineAnswer.asked(asked, of: native) }
        onlineAnswers.keep(fresh, for: asked)
        return fresh
    }

    @ObservationIgnored private var visualizers = 0
    @ObservationIgnored private var loudnessListeners = 0
    @ObservationIgnored private(set) var heardRms: Float = 0

    func visualize(_ enabled: Bool) {
        visualizers = max(visualizers + (enabled ? 1 : -1), 0)
        tapHoldersChanged()
    }

    func hearLoudness(_ enabled: Bool) {
        loudnessListeners = max(loudnessListeners + (enabled ? 1 : -1), 0)
        tapHoldersChanged()
    }

    private var visualizersWatching: Bool {
        visualizers > 0 && foreground && library.transport == .playing
    }

    func tapHoldersChanged() {
        if !visualizersWatching, !samples.isEmpty { samples = [] }
        guard visualizersWatching || loudnessListeners > 0 else {
            visualization?.cancel()
            visualization = nil
            heardRms = 0
            return
        }
        guard visualization == nil else { return }
        visualization = Task { [weak self] in
            guard let native = try? await self?.library.player() else { return }
            while !Task.isCancelled {
                var wait = self?.visualizersWatching == true ? Features.visualizerFrame : Features.loudnessFrame
                do {
                    let heard = try await Work.aside { HeardSignal(try native.visualizer(enabled: true)) }
                    self?.heard(heard)
                } catch {
                    wait = Features.visualizerRetry
                }
                try? await Task.sleep(for: wait)
            }
            _ = try? await Work.aside { try native.visualizer(enabled: false) }
        }
    }

    private func heard(_ signal: HeardSignal) {
        heardRms = signal.rms
        if visualizersWatching { samples = signal.samples }
    }
}

struct HeardSignal: Sendable {
    let samples: [Float]
    let rms: Float

    init(_ samples: [Float]) {
        self.samples = samples
        let power = samples.isEmpty ? 0 : samples.reduce(0) { $0 + $1 * $1 } / Float(samples.count)
        rms = power.squareRoot()
    }
}

private struct SecretsRead: Sendable {
    let providers: Result<ProviderAccounts, any Swift.Error>
    let scrobbling: Result<ScrobbleAccount, any Swift.Error>
    let recognition: Result<String, any Swift.Error>
    let sync: Result<SyncConfig, any Swift.Error>
}

struct RefusedLanding {
    let until: Date
    let tries: Int
}

@MainActor
@Observable
final class ArtStamp {
    var value = 0
}
