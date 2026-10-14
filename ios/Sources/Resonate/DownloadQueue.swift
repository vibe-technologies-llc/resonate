import Foundation
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

extension DownloadAccount {
    var isReady: Bool {
        switch provider {
        case .monochrome, .hifiApi: true
        case .subsonic: !server.isEmpty && !user.isEmpty
        case .tidal: !clientId.isEmpty && !refreshToken.isEmpty
        }
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

    var jobs: [DownloadJob] = []
    var awaitingFolder: [SearchSong] = []
    var needsProvider = false
    private var folderId: String?

    @ObservationIgnored private unowned let library: Library
    @ObservationIgnored private let settings = UserDefaults(suiteName: "library-features") ?? .standard
    @ObservationIgnored private var polling: Task<Void, Never>?
    @ObservationIgnored private var created: [UInt64: String] = [:]
    @ObservationIgnored private var paused = false
    @ObservationIgnored private lazy var background = BackgroundHold(name: "Resonate downloads") { [weak self] in
        self?.expire()
    }

    init(library: Library) {
        self.library = library
        folderId = settings.string(forKey: Self.folderKey)
    }

    var active: [DownloadJob] { jobs.filter { !$0.state.settled } }

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
            needsProvider = true
            return
        }
        needsProvider = false
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

    func retry(_ id: UInt64) {
        guard let job = jobs.first(where: { $0.id == id }), job.state.settled else { return }
        jobs.removeAll { $0.id == id }
        begin([job.song], into: job.folder)
    }

    func clearSettled() {
        jobs.removeAll { $0.state.settled }
    }

    func foregroundChanged(_ active: Bool) {
        guard active, paused else { return }
        paused = false
        Task {
            guard let native = try? await library.player() else { return }
            advance(native)
        }
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
        background.hold(running)
        if running { follow(native) }
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
    private let name: String
    private let expired: @MainActor () -> Void
    private var identifier = UIBackgroundTaskIdentifier.invalid

    init(name: String, expired: @escaping @MainActor () -> Void = {}) {
        self.name = name
        self.expired = expired
    }

    func hold(_ busy: Bool) {
        if busy, identifier == .invalid {
            identifier = UIApplication.shared.beginBackgroundTask(withName: name, expirationHandler: Self.expiry(of: self))
        } else if !busy, identifier != .invalid {
            UIApplication.shared.endBackgroundTask(identifier)
            identifier = .invalid
        }
    }

    private func expire() {
        expired()
        hold(false)
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
