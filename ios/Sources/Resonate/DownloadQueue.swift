import BackgroundTasks
import Foundation
import OSLog
import Observation
import ResonateNative
import UIKit

enum DownloadState: Equatable {
    case waiting
    case finding
    case downloading
    case validating
    case done(Track)
    case failed(String)
    case cancelled

    var inFlight: Bool {
        switch self {
        case .finding, .downloading, .validating: true
        default: false
        }
    }

    var settled: Bool {
        switch self {
        case .done, .failed, .cancelled: true
        default: false
        }
    }
}

struct DownloadJob: Identifiable, Equatable {
    let id: UInt64
    let song: SearchSong
    let folder: MusicFolder
    var state = DownloadState.waiting
    var received: UInt64 = 0
    var total: UInt64?
    var provider: MusicProvider?

    var fraction: Double? {
        guard let total, total > 0 else { return nil }
        return min(Double(received) / Double(total), 1)
    }

    mutating func follow(_ progress: ResonateNative.DownloadProgress) {
        received = progress.received
        total = progress.total ?? total
        provider = progress.provider ?? provider
        guard state.inFlight else { return }
        switch progress.phase {
        case .finding: state = .finding
        case .downloading: state = .downloading
        case .validating: state = .validating
        case .done, .failed, .cancelled: return
        }
    }
}

enum ProviderReadiness: Equatable {
    case ready
    case missingServer
    case missingLogin
    case missingClient
    case needsSignIn
}

extension DownloadAccount {
    var readiness: ProviderReadiness {
        switch provider {
        case .monochrome, .hifiApi: .ready
        case .subsonic where Self.isBlank(server): .missingServer
        case .subsonic where Self.isBlank(user) || password.isEmpty: .missingLogin
        case .subsonic: .ready
        case .tidal where Self.isBlank(clientId): .missingClient
        case .tidal where Self.isBlank(refreshToken): .needsSignIn
        case .tidal: .ready
        }
    }

    var usesItsOwnServer: Bool { !Self.isBlank(server) }

    private static func isBlank(_ value: String) -> Bool {
        value.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }
}

extension ResonateNative.Error {
    var isCancelledDownload: Bool {
        if case .DownloadCancelled = self { return true }
        return false
    }
}

@MainActor
@Observable
final class DownloadQueue {
    private static let runningAtOnce = 2
    private static let pollEvery: Duration = .milliseconds(250)
    private static let journalKey = "unfinished-download"
    private static let folderKey = "download-folder"
    private static let wantsAutomaticallyKey = "download-wants-automatically"
    private static let wantsAskedAtLeast: Duration = .seconds(30)
    private static let wantsAskedEvery: Duration = .seconds(30 * 60)
    private static let progressSteps: Int64 = 1000
    private static let erasureWaitTicks = 40

    var jobs: [DownloadJob] = []
    var awaitingFolder: [SearchSong] = []
    var missedProvider = false
    var asksForWants: Bool {
        didSet {
            settings.set(asksForWants, forKey: Self.wantsAutomaticallyKey)
            wantsChanged()
        }
    }
    private var folderId: String?

    @ObservationIgnored private unowned let library: Library
    @ObservationIgnored private let settings = UserDefaults(suiteName: "library-features") ?? .standard
    @ObservationIgnored private var polling: Task<Void, Never>?
    @ObservationIgnored private var created: [UInt64: String] = [:]
    @ObservationIgnored private var paused = false
    @ObservationIgnored private var wantsPoll: Task<Void, Never>?
    @ObservationIgnored private var askedOnItsOwn: [String: ContinuousClock.Instant] = [:]
    @ObservationIgnored private var batch: Set<UInt64> = []
    @ObservationIgnored private lazy var background = BackgroundHold(
        name: "Downloading songs",
        continued: BackgroundHold.downloads
    ) { [weak self] in
        self?.expire()
    }

    init(library: Library) {
        self.library = library
        folderId = settings.string(forKey: Self.folderKey)
        asksForWants = settings.object(forKey: Self.wantsAutomaticallyKey) == nil || settings.bool(forKey: Self.wantsAutomaticallyKey)
    }

    var active: [DownloadJob] { jobs.filter { !$0.state.settled } }

    var needsProvider: Bool { missedProvider && library.features.downloadAccounts.isEmpty }

    var choosingFolder: Bool {
        get { !awaitingFolder.isEmpty }
        set { if !newValue { awaitingFolder = [] } }
    }

    var rememberedFolder: MusicFolder? {
        guard let id = folderId else { return nil }
        return library.folderList.first { $0.id == id && $0.readable && $0.writable }
    }

    func holds(_ song: SearchSong) -> Bool {
        active.contains { $0.song.id == song.id }
    }

    func recover() {
        let left = settings.stringArray(forKey: Self.journalKey) ?? settings.string(forKey: Self.journalKey).map { [$0] } ?? []
        for uri in left { library.folders.delete(uri) }
        settings.removeObject(forKey: Self.journalKey)
    }

    func enqueue(_ songs: [SearchSong]) {
        let wanted = songs.filter { !holds($0) }
        guard !wanted.isEmpty else { return }
        guard !library.features.downloadAccounts.isEmpty else {
            missedProvider = true
            return
        }
        missedProvider = false
        guard let folder = rememberedFolder else {
            awaitingFolder = wanted
            return
        }
        begin(wanted, into: folder)
    }

    func choose(_ folder: MusicFolder, remember: Bool) {
        let songs = awaitingFolder
        awaitingFolder = []
        if remember {
            settings.set(folder.id, forKey: Self.folderKey)
            folderId = folder.id
            wantsChanged()
        }
        begin(songs, into: folder)
    }

    func forgetFolder() {
        settings.removeObject(forKey: Self.folderKey)
        folderId = nil
    }

    func cancel(_ id: UInt64) {
        guard let job = jobs.first(where: { $0.id == id }), !job.state.settled else { return }
        if job.state == .waiting { update(id) { $0.state = .cancelled } }
        Task {
            guard let native = try? await library.player() else { return }
            _ = try? await Work.aside { native.cancelDownload(job: id) }
        }
    }

    func stopForErasing() async {
        wantsPoll?.cancel()
        wantsPoll = nil
        paused = true
        for job in jobs where job.state == .waiting { update(job.id) { $0.state = .cancelled } }
        if let native = library.native {
            for job in jobs where job.state.inFlight { native.cancelDownload(job: job.id) }
        }
        for _ in 0..<Self.erasureWaitTicks where jobs.contains(where: { $0.state.inFlight }) {
            try? await Task.sleep(for: Self.pollEvery)
        }
        polling?.cancel()
        polling = nil
    }

    func retry(_ id: UInt64) {
        guard let job = jobs.first(where: { $0.id == id }), job.state.settled else { return }
        jobs.removeAll { $0.id == id }
        begin([job.song], into: job.folder)
    }

    func clearSettled() {
        jobs.removeAll { $0.state.settled }
    }

    func wantsChanged() {
        wantsPoll?.cancel()
        wantsPoll = nil
        askForWants()
    }

    func foregroundChanged(_ active: Bool) {
        if active {
            askForWants()
        } else {
            wantsPoll?.cancel()
            wantsPoll = nil
        }
        guard active, paused else { return }
        paused = false
        Task {
            guard let native = try? await library.player() else { return }
            advance(native)
        }
    }

    private func askForWants() {
        guard wantsPoll == nil, asksForWants, library.features.foreground else { return }
        wantsPoll = Task { [weak self] in
            while !Task.isCancelled {
                guard let native = try? await self?.library.player(),
                    let next = (try? await Work.aside({ try native.nextWantedDue() })) ?? nil
                else {
                    self?.wantsPoll = nil
                    return
                }
                try? await Task.sleep(for: DownloadQueue.waitForWants(dueAt: next))
                guard !Task.isCancelled, let self else { return }
                await askDueWants(native)
            }
        }
    }

    private static func waitForWants(dueAt: UInt64) -> Duration {
        let seconds = Double(dueAt) - Date().timeIntervalSince1970
        let wait = Duration.milliseconds(Int64(max(seconds, 0) * 1000))
        return min(max(wait, wantsAskedAtLeast), wantsAskedEvery)
    }

    private func askDueWants(_ native: MobilePlayer) async {
        guard asksForWants, library.features.online, !library.features.downloadAccounts.isEmpty, let folder = rememberedFolder,
            let due = try? await Work.aside({ try native.pollWantedDue() })
        else { return }
        let now = ContinuousClock.now
        askedOnItsOwn = askedOnItsOwn.filter { now - $0.value < Self.wantsAskedEvery }
        let songs = due.map(\.song).filter { askedOnItsOwn[$0.id] == nil && !holds($0) }
        for song in songs { askedOnItsOwn[song.id] = now }
        begin(songs, into: folder)
    }

    private func begin(_ songs: [SearchSong], into folder: MusicFolder) {
        guard !songs.isEmpty else { return }
        Task {
            do {
                let native = try await library.player()
                let ids = try await Work.aside { songs.map { native.beginDownload(song: $0) } }
                jobs += zip(ids, songs).map { DownloadJob(id: $0, song: $1, folder: folder) }
                advance(native)
            } catch {
                library.report(error)
            }
        }
    }

    private func advance(_ native: MobilePlayer) {
        if !paused {
            let free = Self.runningAtOnce - jobs.filter(\.state.inFlight).count
            for job in jobs.filter({ $0.state == .waiting }).prefix(max(free, 0)) {
                update(job.id) { $0.state = .finding }
                Task { await run(job, native) }
            }
        }
        let running = jobs.contains { $0.state.inFlight }
        if running {
            batch.formUnion(jobs.filter { !$0.state.settled }.map(\.id))
        } else {
            batch = []
        }
        background.hold(running)
        showProgress()
        if running { follow(native) }
    }

    private func showProgress() {
        let held = jobs.filter { batch.contains($0.id) }
        guard !held.isEmpty else { return }
        let settled = held.filter(\.state.settled).count
        let done = held.reduce(Int64(0)) { sum, job in
            sum + (job.state.settled ? Self.progressSteps : Int64((job.fraction ?? 0) * Double(Self.progressSteps)))
        }
        background.progress(ContinuedProgress(
            done: done,
            total: Int64(held.count) * Self.progressSteps,
            subtitle: "\(settled) of \(counted(held.count, "song"))"
        ))
    }

    private func run(_ job: DownloadJob, _ native: MobilePlayer) async {
        let id = job.id
        let folder = job.folder.id
        let accounts = library.features.downloadAccounts
        let folders = library.folders
        do {
            let (offer, renewed) = try await Work.aside {
                (try native.findDownload(job: id, accounts: accounts), native.refreshedTidalToken())
            }
            if let renewed { library.features.keepRenewedTidalToken(renewed) }
            update(id) {
                $0.provider = offer.provider
                $0.total = offer.total ?? $0.total
                $0.state = .downloading
            }

            let (uri, descriptor) = try await Work.aside { try folders.fresh(in: folder, named: offer.name) }
            created[id] = uri
            journal { $0.append(uri) }
            _ = try await Work.aside { try native.downloadOffer(job: id, descriptor: descriptor) }

            update(id) { $0.state = .validating }
            let track = try await Work.aside { try native.indexDownload(job: id, uri: uri, name: offer.name) }
            created[id] = nil
            journal { $0.removeAll { $0 == uri } }

            update(id) { $0.state = .done(track) }
            landed(track)
        } catch {
            await discard(id, folders)
            let cancelled = (error as? ResonateNative.Error)?.isCancelledDownload == true || error is CancellationError
            update(id) { $0.state = cancelled ? .cancelled : .failed(explained(error)) }
        }
        advance(native)
    }

    private func discard(_ id: UInt64, _ folders: Folders) async {
        guard let uri = created.removeValue(forKey: id) else { return }
        _ = try? await Work.aside { folders.delete(uri) }
        journal { $0.removeAll { $0 == uri } }
    }

    private func landed(_ track: Track) {
        let features = library.features
        features.enrichment.landed(track.id)
        Task {
            await library.refresh()
            await features.rereadWanted()
            await features.refreshChangedArt()
        }
    }

    private func follow(_ native: MobilePlayer) {
        guard polling == nil else { return }
        polling = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(for: DownloadQueue.pollEvery)
                guard let self else { return }
                let ids = jobs.filter(\.state.inFlight).map(\.id)
                guard !ids.isEmpty else {
                    polling = nil
                    return
                }
                let read = (try? await Work.aside {
                    ids.compactMap { id in native.downloadProgress(job: id).map { (id, $0) } }
                }) ?? []
                for (id, progress) in read { update(id) { $0.follow(progress) } }
                showProgress()
            }
        }
    }

    private func expire() {
        paused = true
        guard let native = library.native else { return }
        for job in jobs where job.state.inFlight {
            native.cancelDownload(job: job.id)
        }
    }

    private func update(_ id: UInt64, _ change: (inout DownloadJob) -> Void) {
        guard let index = jobs.firstIndex(where: { $0.id == id }) else { return }
        change(&jobs[index])
    }

    private func journal(_ change: (inout [String]) -> Void) {
        var uris = settings.stringArray(forKey: Self.journalKey) ?? []
        change(&uris)
        if uris.isEmpty {
            settings.removeObject(forKey: Self.journalKey)
        } else {
            settings.set(uris, forKey: Self.journalKey)
        }
    }
}

@MainActor
final class BackgroundHold {
    static let downloads = "org.resonate.ios.downloads"
    static let transfers = "org.resonate.ios.transfers"
    private static var asked: [String: BackgroundHold] = [:]

    private let name: String
    private let continued: String?
    private let expired: @MainActor () -> Void
    private var identifier = UIBackgroundTaskIdentifier.invalid
    private var request: String?
    private var task: BGContinuedProcessingTask?
    private var shown = ContinuedProgress()

    init(name: String, continued: String? = nil, expired: @escaping @MainActor () -> Void = {}) {
        self.name = name
        self.continued = continued
        self.expired = expired
    }

    static func register() {
        for prefix in [downloads, transfers] {
            BGTaskScheduler.shared.register(forTaskWithIdentifier: "\(prefix).*", using: .main, launchHandler: launched)
        }
    }

    func hold(_ busy: Bool) {
        if busy {
            if identifier == .invalid, task == nil {
                identifier = UIApplication.shared.beginBackgroundTask(withName: name, expirationHandler: Self.expiry(of: self))
            }
            askToContinue()
        } else {
            endPlain()
            finishContinued(success: true)
        }
    }

    func progress(_ progress: ContinuedProgress) {
        shown = progress
        show()
    }

    private func askToContinue() {
        guard let continued, request == nil, task == nil, UIApplication.shared.applicationState == .active else { return }
        let id = "\(continued).\(UUID().uuidString)"
        let asking = BGContinuedProcessingTaskRequest(identifier: id, title: name, subtitle: shown.subtitle)
        asking.strategy = .fail
        do {
            try BGTaskScheduler.shared.submit(asking)
            request = id
            Self.asked[id] = self
        } catch {
            Logger(subsystem: "org.resonate.ios", category: "background")
                .notice("\(self.name, privacy: .public) was not given time to continue: \(error, privacy: .public)")
        }
    }

    private func began(_ given: BGContinuedProcessingTask) {
        request = nil
        task = given
        given.expirationHandler = Self.expiry(of: self)
        endPlain()
        show()
    }

    private func show() {
        guard let task else { return }
        task.progress.totalUnitCount = max(shown.total, 1)
        task.progress.completedUnitCount = min(shown.done, max(shown.total, 1))
        task.updateTitle(name, subtitle: shown.subtitle)
    }

    private func endPlain() {
        guard identifier != .invalid else { return }
        UIApplication.shared.endBackgroundTask(identifier)
        identifier = .invalid
    }

    private func finishContinued(success: Bool) {
        if let request { Self.asked[request] = nil }
        request = nil
        task?.setTaskCompleted(success: success)
        task = nil
    }

    private func expire() {
        expired()
        endPlain()
        finishContinued(success: false)
    }

    private static func started(_ given: BGTask) {
        guard let given = given as? BGContinuedProcessingTask,
            let hold = asked.removeValue(forKey: given.identifier),
            hold.request == given.identifier
        else {
            given.setTaskCompleted(success: false)
            return
        }
        hold.began(given)
    }

    nonisolated private static func launched(_ given: BGTask) {
        nonisolated(unsafe) let deliveredOnTheMainQueue = given
        MainActor.assumeIsolated { started(deliveredOnTheMainQueue) }
    }

    nonisolated private static func expiry(of hold: BackgroundHold) -> @Sendable () -> Void {
        { [weak hold] in
            if Thread.isMainThread {
                MainActor.assumeIsolated { hold?.expire() }
            } else {
                Task { @MainActor in hold?.expire() }
            }
        }
    }
}

struct ContinuedProgress: Equatable {
    var done: Int64 = 0
    var total: Int64 = 0
    var subtitle = ""
}
