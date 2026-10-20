import BackgroundTasks
import Foundation
import OSLog
import Observation
import ResonateLogic
import ResonateNative
import UIKit

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
    private static let folderKey = "download-folder"
    private static let wantsAutomaticallyKey = "download-wants-automatically"
    private static let wantsAskedAtLeast: Duration = .seconds(30)
    private static let wantsAskedEvery: Duration = .seconds(30 * 60)
    private static let progressSteps: Int64 = 1000
    private static let erasureWaitTicks = 40

    var jobs: [DownloadJob] = [] {
        didSet { findHeld() }
    }
    private(set) var held: Set<String> = []
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
    @ObservationIgnored private let journal = DownloadJournal()
    @ObservationIgnored private var polling: Task<Void, Never>?
    @ObservationIgnored private var created: [UInt64: String] = [:]
    @ObservationIgnored private var paused = false
    @ObservationIgnored private var wantsPoll: Task<Void, Never>?
    @ObservationIgnored private var settling: Task<Void, Never>?
    @ObservationIgnored private var landedSince = false
    @ObservationIgnored private var wantsGeneration = 0
    @ObservationIgnored private var askedOnItsOwn: [String: ContinuousClock.Instant] = [:]
    @ObservationIgnored private var batch: Set<UInt64> = []
    @ObservationIgnored private var beginning: Set<String> = [] {
        didSet { findHeld() }
    }
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
        held.contains(song.id)
    }

    private func findHeld() {
        let found = beginning.union(jobs.lazy.filter { !$0.state.settled }.map(\.song.id))
        if found != held { held = found }
    }

    func recover(_ native: MobilePlayer) {
        let left = journal.uris
        guard !left.isEmpty else { return }
        let folders = library.folders
        Task { [weak self] in
            let kept = try? await Work.aside {
                let indexed = Set(try native.songUris())
                return left.filter { !indexed.contains($0) && !folders.delete($0) }
            }
            guard let self, let kept else { return }
            journal.settleRecovery(left: left, kept: kept)
        }
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
        wantsGeneration += 1
        paused = true
        landedSince = false
        for job in jobs where job.state == .waiting { update(job.id) { $0.state = .cancelled } }
        if let native = library.native {
            let running = jobs.filter { $0.state.inFlight }.map(\.id)
            _ = try? await Work.aside { running.forEach { native.cancelDownload(job: $0) } }
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
        wantsGeneration += 1
        askForWants()
    }

    func foregroundChanged(_ active: Bool) {
        if active {
            askForWants()
        } else {
            wantsPoll?.cancel()
            wantsPoll = nil
            wantsGeneration += 1
        }
        guard active, paused else { return }
        paused = false
        Task {
            guard let native = try? await library.player() else { return }
            advance(native)
        }
    }

    private func askForWants() {
        guard wantsPoll == nil, mayAskForWants, library.features.foreground else { return }
        wantsGeneration += 1
        let generation = wantsGeneration
        wantsPoll = Task { [weak self] in await self?.keepAskingForWants(generation) }
    }

    private var mayAskForWants: Bool {
        let features = library.features
        return asksForWants && features.online && features.unmetered && !ProcessInfo.processInfo.isLowPowerModeEnabled
            && !features.downloadAccounts.isEmpty && rememberedFolder != nil
    }

    private func keepAskingForWants(_ generation: Int) async {
        defer {
            if wantsGeneration == generation { wantsPoll = nil }
        }
        while !Task.isCancelled, mayAskForWants {
            guard let native = try? await library.player(),
                let next = (try? await Work.aside({ try native.nextWantedDue() })) ?? nil
            else { return }
            try? await Task.sleep(for: DownloadQueue.waitForWants(dueAt: next))
            guard !Task.isCancelled else { return }
            await askDueWants(native)
        }
    }

    private static func waitForWants(dueAt: UInt64) -> Duration {
        let seconds = Double(dueAt) - Date().timeIntervalSince1970
        let wait = Duration.milliseconds(Int64(max(seconds, 0) * 1000))
        return min(max(wait, wantsAskedAtLeast), wantsAskedEvery)
    }

    private func askDueWants(_ native: MobilePlayer) async {
        guard mayAskForWants, let folder = rememberedFolder,
            let due = try? await Work.aside({ try native.pollWantedDue() })
        else { return }
        let now = ContinuousClock.now
        askedOnItsOwn = askedOnItsOwn.filter { now - $0.value < Self.wantsAskedEvery }
        let songs = due.map(\.song).filter { askedOnItsOwn[$0.id] == nil && !holds($0) }
        for song in songs { askedOnItsOwn[song.id] = now }
        begin(songs, into: folder)
    }

    private func begin(_ asked: [SearchSong], into folder: MusicFolder) {
        var seen: Set<String> = []
        let songs = asked.filter { !holds($0) && seen.insert($0.id).inserted }
        guard !songs.isEmpty else { return }
        beginning.formUnion(songs.map(\.id))
        Task {
            defer { beginning.subtract(songs.map(\.id)) }
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
            let (found, renewed) = try await Work.aside {
                let found = Result { try native.findDownload(job: id, accounts: accounts) }
                return (found, native.refreshedTidalToken())
            }
            if let renewed { library.features.keepRenewedTidalToken(renewed) }
            let offer = try found.get()
            update(id) {
                $0.provider = offer.provider
                $0.total = offer.total ?? $0.total
                $0.state = .downloading
            }

            let (uri, descriptor) = try await Work.aside { try folders.fresh(in: folder, named: offer.name) }
            created[id] = uri
            journal.change { $0.append(uri) }
            _ = try await Work.aside { try native.downloadOffer(job: id, descriptor: descriptor) }

            update(id) { $0.state = .validating }
            let track = try await Work.aside { try native.indexDownload(job: id, uri: uri, name: offer.name) }
            created[id] = nil
            folders.written(uri)
            journal.change { $0.removeAll { $0 == uri } }

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
        let gone = (try? await Work.aside { folders.delete(uri) }) ?? false
        if gone { journal.change { $0.removeAll { $0 == uri } } }
    }

    private func landed(_ track: Track) {
        let features = library.features
        features.enrichment.landed(track.id)
        landedSince = true
        guard settling == nil else { return }
        settling = Task { [weak self] in
            await self?.settleLandings()
        }
    }

    private func settleLandings() async {
        let features = library.features
        while landedSince {
            landedSince = false
            await library.refresh()
            await features.rereadWanted()
            await features.refreshChangedArt()
        }
        settling = nil
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
        var changed = jobs[index]
        change(&changed)
        guard changed != jobs[index] else { return }
        jobs[index] = changed
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
    private var machine: HoldMachine
    private var task: BGContinuedProcessingTask?
    private var shown = ContinuedProgress()

    init(name: String, continued: String? = nil, expired: @escaping @MainActor () -> Void = {}) {
        self.name = name
        self.continued = continued
        self.expired = expired
        machine = HoldMachine(continues: continued != nil)
    }

    static func register() {
        for prefix in [downloads, transfers] {
            BGTaskScheduler.shared.register(forTaskWithIdentifier: "\(prefix).*", using: .main, launchHandler: launched)
        }
    }

    func hold(_ busy: Bool) {
        let active = UIApplication.shared.applicationState == .active
        let continued = continued
        perform(machine.hold(busy, active: active) { "\(continued ?? "").\(UUID().uuidString)" })
    }

    func progress(_ progress: ContinuedProgress) {
        shown = progress
        show()
    }

    private func perform(_ effects: [HoldEffect]) {
        for effect in effects {
            switch effect {
            case .beginPlain:
                identifier = UIApplication.shared.beginBackgroundTask(withName: name, expirationHandler: Self.expiry(of: self))
            case .endPlain:
                guard identifier != .invalid else { continue }
                UIApplication.shared.endBackgroundTask(identifier)
                identifier = .invalid
            case .submit(let id):
                submit(id)
            case .forgetAsked(let id):
                Self.asked[id] = nil
            case .complete(let success):
                task?.setTaskCompleted(success: success)
                task = nil
            case .notifyExpired:
                expired()
            case .show:
                show()
            }
        }
    }

    private func submit(_ id: String) {
        let asking = BGContinuedProcessingTaskRequest(identifier: id, title: name, subtitle: shown.subtitle)
        asking.strategy = .fail
        do {
            try BGTaskScheduler.shared.submit(asking)
            machine.submitted(id, accepted: true)
            Self.asked[id] = self
        } catch {
            Logger(subsystem: "org.resonate.ios", category: "background")
                .notice("\(self.name, privacy: .public) was not given time to continue: \(error, privacy: .public)")
        }
    }

    private func began(_ given: BGContinuedProcessingTask) {
        task = given
        given.expirationHandler = Self.expiry(of: self)
        perform(machine.began())
    }

    private func show() {
        guard let task else { return }
        task.progress.totalUnitCount = max(shown.total, 1)
        task.progress.completedUnitCount = min(shown.done, max(shown.total, 1))
        task.updateTitle(name, subtitle: shown.subtitle)
    }

    private func expire() {
        perform(machine.expired())
    }

    private static func started(_ given: BGTask) {
        guard let given = given as? BGContinuedProcessingTask,
            let hold = asked.removeValue(forKey: given.identifier),
            hold.machine.accepts(given.identifier)
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
