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

@MainActor
@Observable
final class Features {
    private static let scrobbleEvery: Duration = .seconds(30)
    private static let syncForeground: Duration = .seconds(2)
    private static let syncBackground: Duration = .seconds(30)
    private static let landingPause: TimeInterval = 60
    private static let visualizerFrame: Duration = .milliseconds(33)
    private static let artworkBytes = 32 * 1024 * 1024
    nonisolated private static let largestEdge: CGFloat = 1024
    nonisolated private static let wantedChunk = 500

    @ObservationIgnored private unowned let library: Library
    @ObservationIgnored private let accounts = AccountStore()
    @ObservationIgnored private let settings = UserDefaults(suiteName: "library-features") ?? .standard
    @ObservationIgnored private let pictures: NSCache<NSString, UIImage> = {
        let cache = NSCache<NSString, UIImage>()
        cache.totalCostLimit = artworkBytes
        return cache
    }()

    var tidalAuthorization: TidalAuthorization?
    var tidalBusy = false
    var corrections: [AutoEqDevice] = []
    var correctionBusy = false
    var scrobbleAccount: ScrobbleAccount?
    var scrobbleStatus: String?
    var providerAccount: DownloadAccount?
    var downloading = false
    var downloadedBytes: UInt64 = 0
    var downloadStatus: String?
    var online: Bool
    var statistics: LibraryStatistics?
    var lyrics: LyricSheet?
    var lyricFrame: LyricFrame?
    var lyricsLoading = false
    var recognitionKey = ""
    var recognition: [RecognitionCandidate] = []
    var recognitionBusy = false
    var artistAlbumsNext: UInt32?
    var analysis: AnalysisReport?
    var analysisBusy = false
    var analysisProgress: Float?
    var inspector: Inspector?
    var artist: ArtistInformation?
    var playlists: [MusicPlaylist] = []
    var playlistRows: [Track] = []
    var openPlaylist: UInt64?
    var onlineAlbums: [SearchAlbum] = []
    var onlineArtists: [SearchArtist] = []
    var onlineReleases: [SearchRelease] = []
    var onlineSongs: [SearchSong] = []
    var onlineCollection: OnlineCollection?
    var wanted: [WantedSong] = []
    var searching = false
    var enriching = false
    var samples: [Float] = []
    var artworkGeneration = 0
    var syncConfig: SyncConfig?
    var syncStatus: SyncStatus?
    var syncHandoffs: [HandedOff] = []
    var syncFolder: String?
    var syncNotice: String?
    var playlistSync: PlaylistSync?
    var transferring = false

    @ObservationIgnored private var artistRequest = 0
    @ObservationIgnored private var analysisRequest = 0
    @ObservationIgnored private var recognitionRequest = 0
    @ObservationIgnored private var lyricRequest = 0
    @ObservationIgnored private var searchRequest = 0
    @ObservationIgnored private var submitting: Task<Void, Never>?
    @ObservationIgnored private var visualization: Task<Void, Never>?
    @ObservationIgnored private var syncPolling: Task<Void, Never>?
    @ObservationIgnored private var network: NWPathMonitor?
    @ObservationIgnored private var unmetered = false
    @ObservationIgnored var foreground = true
    @ObservationIgnored private var landingRefusedUntil = Date.distantPast
    @ObservationIgnored private var backgroundWork: UIBackgroundTaskIdentifier = .invalid

    init(library: Library) {
        self.library = library
        online = settings.object(forKey: "online") == nil ? true : settings.bool(forKey: "online")
        syncFolder = settings.string(forKey: "sync-folder")
    }

    private func work(_ action: @escaping @MainActor (MobilePlayer) async throws -> Void) {
        Task {
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
        providerAccount = accounts.read()
        scrobbleAccount = accounts.readScrobbling()
        recognitionKey = accounts.readRecognitionKey()
        syncConfig = accounts.readSync()
        native.configureOnline(enabled: online)
        for key in ["unfinished-download", "unfinished-sync-landing"] {
            if let left = settings.string(forKey: key) {
                library.folders.delete(left)
                settings.removeObject(forKey: key)
            }
        }
        submitting?.cancel()
        submitting = Task { [weak self] in await self?.submitScrobbles(native) }
        watchNetwork()
        syncPolling?.cancel()
        syncPolling = Task { [weak self] in await self?.pollSync(native) }
        work { [weak self] native in try await self?.applySync(native) }
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
                if count > 0 { scrobbleStatus = "Submitted \(count) listens" }
            } catch {
                scrobbleStatus = explained(error)
            }
        }
    }

    func tick(_ native: MobilePlayer, _ snapshot: Snapshot) async {
        let sheet = lyrics
        let watchingDownload = downloading
        let watchingAnalysis = analysisBusy
        guard let (frame, bytes, progress) = try? await Work.aside({
            let frame = sheet.flatMap { sheet in
                snapshot.current?.id == sheet.trackId
                    ? native.lyricFrame(id: sheet.trackId, positionMs: snapshot.positionMs)
                    : nil
            }
            return (
                frame,
                watchingDownload ? native.downloadBytes() : nil,
                watchingAnalysis ? native.analysisProgress() : nil
            )
        }) else { return }
        if let frame { lyricFrame = frame }
        if let bytes { downloadedBytes = bytes }
        if watchingAnalysis { analysisProgress = progress }
    }

    func setOnline(_ enabled: Bool) {
        work { [weak self] native in
            native.configureOnline(enabled: enabled)
            await MainActor.run {
                self?.online = enabled
                self?.settings.set(enabled, forKey: "online")
            }
            try await self?.applySync(native)
        }
    }

    func saveSync(_ config: SyncConfig) {
        work { [weak self] native in
            guard let self else { return }
            try await MainActor.run {
                try accounts.writeSync(config)
                syncConfig = config
            }
            try await applySync(native)
        }
    }

    func joinSync(_ code: String) {
        guard var config = syncConfig else { return }
        config.key = code.trimmingCharacters(in: .whitespacesAndNewlines)
        saveSync(config)
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

    func fetchMissing(_ id: UInt64, from device: String) {
        work { [weak self] native in
            let asked = try await Work.aside { try native.syncFetchMissing(playlist: id, device: device) }
            self?.library.notice(
                asked > 0 ? "Asking \(device) for \(counted(Int(asked), "song"))" : "\(device) has nothing this playlist lacks"
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
        let device = handoff.device
        work { [weak self] native in
            let loaded = try await Work.inOrder { try native.syncContinueFrom(device: device) }
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

    private func applySync(_ native: MobilePlayer) async throws {
        guard let config = syncConfig else { return }
        let enabled = config.enabled && online
        let free = unmetered
        do {
            let status = try await Work.inOrder {
                let status: SyncStatus
                if enabled {
                    status = try native.syncStart(config: config)
                } else {
                    native.syncStop()
                    status = native.syncStatus()
                }
                native.syncNetwork(unmetered: free)
                return status
            }
            if !status.code.isEmpty, status.code != config.key {
                var kept = config
                kept.key = status.code
                try accounts.writeSync(kept)
                syncConfig = kept
            }
            syncStatus = status
        } catch {
            syncStatus = native.syncStatus()
            throw error
        }
    }

    private func watchNetwork() {
        guard network == nil else { return }
        let monitor = NWPathMonitor()
        monitor.pathUpdateHandler = { [weak self] path in
            let free = path.status == .satisfied && !path.isExpensive && !path.isConstrained
            Task { @MainActor in self?.networkChanged(free) }
        }
        monitor.start(queue: DispatchQueue(label: "org.resonate.network"))
        network = monitor
    }

    private func networkChanged(_ free: Bool) {
        guard free != unmetered else { return }
        unmetered = free
        work { native in native.syncNetwork(unmetered: free) }
    }

    private func pollSync(_ native: MobilePlayer) async {
        var arrivals: UInt64 = 0
        while !Task.isCancelled {
            try? await Task.sleep(for: foreground || transferring ? Self.syncForeground : Self.syncBackground)
            do {
                let (status, waiting, handoffs) = try await Work.aside {
                    let status = native.syncStatus()
                    let waiting = try native.syncReceived()
                    let handoffs = status.standing == .off ? [] : try native.syncHandoffs()
                    return (status, waiting, handoffs)
                }
                syncStatus = status
                transferring = status.moving > 0 || !waiting.isEmpty
                holdTransfers(transferring || downloading)
                if !waiting.isEmpty { await landReceived(native, waiting) }
                syncHandoffs = handoffs
                if arrivals != status.arrivals {
                    arrivals = status.arrivals
                    await library.refresh()
                    playlists = try await Work.aside { try native.playlists() }
                    if let open = openPlaylist, playlists.contains(where: { $0.id == open }) {
                        await reloadPlaylist(native, open)
                    }
                }
            } catch {
                library.report(error)
            }
        }
    }

    private func holdTransfers(_ busy: Bool) {
        if busy, backgroundWork == .invalid {
            backgroundWork = UIApplication.shared.beginBackgroundTask(withName: "Resonate transfers") { [weak self] in
                MainActor.assumeIsolated { self?.holdTransfers(false) }
            }
        } else if !busy, backgroundWork != .invalid {
            UIApplication.shared.endBackgroundTask(backgroundWork)
            backgroundWork = .invalid
        }
    }

    private func writableFolder(_ preferred: String?) -> MusicFolder? {
        let destinations = library.folderList.filter { $0.readable && $0.writable }
        return destinations.first { $0.id == preferred } ?? destinations.first
    }

    private func landReceived(_ native: MobilePlayer, _ waiting: [ReceivedSong]) async {
        guard Date() >= landingRefusedUntil else { return }
        guard let folder = writableFolder(syncFolder) else {
            syncNotice = "Add a music folder with write access to keep songs from your devices"
            return
        }
        let folders = library.folders
        var landed = 0
        for received in waiting {
            guard !Task.isCancelled else { return }
            let name = received.name
            var created: String?
            do {
                let (uri, descriptor) = try folders.fresh(in: folder.id, named: name)
                created = uri
                settings.set(uri, forKey: "unfinished-sync-landing")
                _ = try await Work.aside { try native.syncReceive(id: received.id, descriptor: descriptor, uri: uri) }
                created = nil
                settings.removeObject(forKey: "unfinished-sync-landing")
                landed += 1
            } catch {
                if let created {
                    folders.delete(created)
                    settings.removeObject(forKey: "unfinished-sync-landing")
                }
                landingRefusedUntil = Date().addingTimeInterval(Self.landingPause)
                syncNotice = explained(error)
                library.report(error)
                break
            }
        }
        if landed > 0 {
            syncNotice = nil
            await library.refresh()
            await refreshWanted(native)
            refreshArtwork()
            library.notice("Kept \(counted(landed, "song")) from your devices")
        }
    }

    func refreshArtwork() {
        pictures.removeAllObjects()
        artworkGeneration += 1
    }

    func artwork(_ track: Track) async -> UIImage? {
        let key = "\(track.id):\(track.artRevision):\(artworkGeneration)" as NSString
        if let kept = pictures.object(forKey: key) { return kept }
        guard let native = try? await library.player() else { return nil }
        let id = track.id
        let decoded = try? await Work.aside { () -> UIImage? in
            guard let bytes = try native.artwork(id: id), let image = UIImage(data: bytes) else { return nil }
            let longest = max(image.size.width, image.size.height)
            guard longest > Features.largestEdge else { return image.preparingForDisplay() ?? image }
            let target = CGSize(
                width: image.size.width * Features.largestEdge / longest,
                height: image.size.height * Features.largestEdge / longest
            )
            return image.preparingThumbnail(of: target) ?? image
        }
        guard let image = decoded ?? nil else { return nil }
        let cost = Int(image.size.width * image.size.height * image.scale * image.scale * 4)
        pictures.setObject(image, forKey: key, cost: cost)
        return image
    }

    func loadPlaylists() {
        work { [weak self] native in
            let found = try await Work.aside { try native.playlists() }
            await MainActor.run { self?.playlists = found }
        }
    }

    func selectPlaylist(_ id: UInt64?) {
        openPlaylist = id
        playlistRows = []
        if let id { work { [weak self] native in await self?.reloadPlaylist(native, id) } }
    }

    func createPlaylist(_ name: String, with tracks: [Track] = []) {
        let ids = tracks.map(\.id)
        editPlaylist { native in
            let id = try native.createPlaylist(name: name)
            if !ids.isEmpty { try native.addToPlaylist(id: id, tracks: ids) }
        }
    }

    func addToPlaylist(_ id: UInt64, _ tracks: [Track]) {
        let ids = tracks.map(\.id)
        editPlaylist { native in try native.addToPlaylist(id: id, tracks: ids) }
    }

    func editPlaylist(_ action: @escaping @Sendable (MobilePlayer) throws -> Void) {
        work { [weak self] native in
            let found = try await Work.inOrder {
                try action(native)
                return try native.playlists()
            }
            await MainActor.run { [weak self] in
                guard let self else { return }
                playlists = found
                if let open = openPlaylist {
                    if found.contains(where: { $0.id == open }) {
                        Task { await self.reloadPlaylist(native, open) }
                    } else {
                        selectPlaylist(nil)
                    }
                }
            }
        }
    }

    private func reloadPlaylist(_ native: MobilePlayer, _ id: UInt64) async {
        do {
            let (rows, found) = try await Work.aside {
                var rows: [Track] = []
                while true {
                    let page = try native.playlistTracks(id: id, offset: UInt32(rows.count), limit: Library.page)
                    rows += page
                    if page.count < Library.page { break }
                }
                return (rows, try native.playlists())
            }
            if openPlaylist == id { playlistRows = rows }
            playlists = found
        } catch {
            library.report(error)
        }
    }

    func loadStatistics(days: UInt32 = 0) {
        work { [weak self] native in
            let found = try await Work.aside { try native.statistics(days: days) }
            await MainActor.run { self?.statistics = found }
        }
    }

    func loadInspector(_ id: UInt64) {
        inspector = nil
        artistRequest += 1
        artist = nil
        work { [weak self] native in
            let found = try await Work.aside { try native.inspector(id: id) }
            await MainActor.run { self?.inspector = found }
        }
    }

    func loadArtist(_ id: UInt64) {
        artistRequest += 1
        let request = artistRequest
        artist = nil
        work { [weak self] native in
            let found = try await Work.aside { try native.artistInformation(id: id) }
            await MainActor.run { if request == self?.artistRequest { self?.artist = found } }
        }
    }

    func onlineArtist(_ id: String) {
        artistRequest += 1
        let request = artistRequest
        artist = nil
        work { [weak self] native in
            let found = try await Work.aside { try native.onlineArtist(id: id) }
            await MainActor.run { if request == self?.artistRequest { self?.artist = found } }
        }
    }

    func loadLyrics(_ id: UInt64, refresh: Bool = false) {
        lyricRequest += 1
        let request = lyricRequest
        lyrics = nil
        lyricFrame = nil
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

    func saveRecognitionKey(_ value: String) {
        do {
            try accounts.writeRecognitionKey(value)
            recognitionKey = value
        } catch {
            library.report(error)
        }
    }

    func recognise(_ id: UInt64) {
        recognitionRequest += 1
        let request = recognitionRequest
        recognition = []
        recognitionBusy = true
        let key = recognitionKey
        work { [weak self] native in
            defer { if request == self?.recognitionRequest { self?.recognitionBusy = false } }
            let found = try await Work.aside { try native.recogniseTrack(id: id, acoustidKey: key) }
            await MainActor.run { [weak self] in
                guard let self, request == recognitionRequest else { return }
                recognition = found
                if found.isEmpty { library.notice("No recording could be identified from this audio") }
            }
        }
    }

    func analyse(_ id: UInt64) {
        recognitionRequest += 1
        recognition = []
        recognitionBusy = false
        analysisRequest += 1
        let request = analysisRequest
        analysis = nil
        analysisBusy = true
        work { [weak self] native in
            defer { if request == self?.analysisRequest { self?.analysisBusy = false } }
            do {
                let report = try await Work.aside { try native.analyseTrack(id: id) }
                await MainActor.run { if request == self?.analysisRequest { self?.analysis = report } }
            } catch {
                let current = await MainActor.run { request == self?.analysisRequest }
                if current { throw error }
            }
        }
    }

    func cancelAnalysis() {
        analysisRequest += 1
        let request = analysisRequest
        analysisBusy = false
        work { [weak self] native in
            let current = await MainActor.run { request == self?.analysisRequest }
            if current { native.cancelAnalysis() }
        }
    }

    func enrich(_ tracks: [Track]) {
        guard !enriching else { return }
        enriching = true
        work { [weak self] native in
            guard let self else { return }
            var failures = 0
            for (index, track) in tracks.enumerated() {
                try Task.checkCancellation()
                library.notice("Enriching \(index + 1)/\(tracks.count): \(track.title)")
                do {
                    _ = try await Work.aside { try native.enrichTrack(id: track.id) }
                } catch {
                    failures += 1
                    library.report(error)
                }
            }
            await library.refresh()
            await MainActor.run {
                refreshArtwork()
                enriching = false
                library.notice("Enrichment finished" + (failures > 0 ? "; \(failures) lookups could not complete" : ""))
            }
        }
    }

    func searchCorrections(_ text: String) {
        correctionBusy = true
        work { [weak self] native in
            defer { self?.correctionBusy = false }
            let found = try await Work.aside { try native.searchAutoEq(text: text) }
            await MainActor.run { self?.corrections = found }
        }
    }

    func applyCorrection(_ device: AutoEqDevice) {
        correctionBusy = true
        work { [weak self] native in
            defer { self?.correctionBusy = false }
            guard let profile = try await Work.aside({ try native.autoEqProfile(id: device.id) }) else { return }
            await MainActor.run {
                self?.library.updatePreferences { current in
                    var next = current
                    next.equaliserEnabled = true
                    next.profileName = profile.name
                    next.profileText = profile.text
                    return next
                }
                self?.library.notice("Applied \(profile.name)")
            }
        }
    }

    func applyProfileText(name: String, text: String) {
        work { [weak self] native in
            let profile = try await Work.aside { try native.parseEqProfile(bytes: Data(text.utf8), name: name) }
            await MainActor.run {
                self?.library.updatePreferences { current in
                    var next = current
                    next.equaliserEnabled = true
                    next.profileName = profile.name
                    next.profileText = profile.text
                    return next
                }
            }
        }
    }

    func saveScrobbling(_ account: ScrobbleAccount, includeHistory: Bool) {
        let previous = scrobbleAccount
        work { [weak self] native in
            try await Work.aside {
                if !account.listenbrainzToken.isEmpty,
                    previous?.listenbrainzToken != account.listenbrainzToken || includeHistory
                {
                    try native.startScrobbling(service: .listenBrainz, includeHistory: includeHistory)
                }
                if !account.lastfmSession.isEmpty,
                    previous?.lastfmSession != account.lastfmSession || includeHistory
                {
                    try native.startScrobbling(service: .lastFm, includeHistory: includeHistory)
                }
            }
            try await MainActor.run { [weak self] in
                guard let self else { return }
                try accounts.writeScrobbling(account)
                scrobbleAccount = account
                scrobbleStatus = "Scrobbling accounts saved"
            }
        }
    }

    func sendFavourites() {
        guard let account = scrobbleAccount else { return }
        work { [weak self] native in
            let count = try await Work.aside { try native.sendFavourites(account: account) }
            await MainActor.run { self?.scrobbleStatus = "Sent \(count) favourite updates" }
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
            try await MainActor.run { [weak self] in
                guard let self else { return }
                try accounts.writeScrobbling(signedIn)
                scrobbleAccount = signedIn
                scrobbleStatus = "Signed in to Last.fm"
            }
            try await Work.aside { try native.startScrobbling(service: .lastFm, includeHistory: false) }
        }
    }

    func authorizeTidal(_ account: DownloadAccount) {
        tidalBusy = true
        work { [weak self] native in
            defer { self?.tidalBusy = false }
            let authorization = try await Work.aside {
                try native.authorizeTidal(clientId: account.clientId, secret: account.clientSecret)
            }
            await MainActor.run { self?.tidalAuthorization = authorization }
            guard let token = try await Work.aside({ try native.finishTidalAuthorization() }) else { return }
            var updated = account
            updated.refreshToken = token
            try await MainActor.run { [weak self] in
                guard let self else { return }
                try accounts.write(updated)
                providerAccount = updated
                tidalAuthorization = nil
                library.notice("Signed in to TIDAL")
            }
        }
    }

    func saveProviderAccount(_ account: DownloadAccount) {
        do {
            try accounts.write(account)
            providerAccount = account
            library.notice("Download provider saved")
        } catch {
            library.report(error)
        }
    }

    func download(_ song: SearchSong, into folder: MusicFolder) {
        guard !downloading, let account = providerAccount else { return }
        downloading = true
        downloadedBytes = 0
        downloadStatus = "Finding \(song.title)…"
        holdTransfers(true)
        let folders = library.folders
        work { [weak self] native in
            guard let self else { return }
            var created: String?
            defer {
                if let created {
                    folders.delete(created)
                    self.settings.removeObject(forKey: "unfinished-download")
                }
                self.downloading = false
                self.holdTransfers(self.transferring)
            }
            do {
                guard let offer = try await Work.aside({ try native.findDownload(song: song, account: account) }) else {
                    await MainActor.run { downloadStatus = "This provider could not find a matching recording" }
                    return
                }
                if let token = native.refreshedTidalToken() {
                    var renewed = account
                    renewed.refreshToken = token
                    try await MainActor.run {
                        try accounts.write(renewed)
                        providerAccount = renewed
                    }
                }
                let (uri, descriptor) = try folders.fresh(in: folder.id, named: offer.name)
                created = uri
                await MainActor.run {
                    settings.set(uri, forKey: "unfinished-download")
                    downloadStatus = "Downloading \(song.title)…"
                }
                let count = try await Work.aside {
                    let count = try native.downloadOffer(id: offer.id, descriptor: descriptor)
                    _ = try native.indexDownload(uri: uri, name: offer.name, song: song)
                    return count
                }
                created = nil
                await refreshWanted(native)
                await MainActor.run {
                    settings.removeObject(forKey: "unfinished-download")
                    downloadStatus = "Downloaded \(offer.name) · \(count / 1024) KiB"
                }
                await library.refresh()
                await MainActor.run { refreshArtwork() }
            } catch {
                await MainActor.run { downloadStatus = explained(error) }
                throw error
            }
        }
    }

    func cancelDownload() {
        work { native in native.cancelDownload() }
    }

    func clearCollection() {
        onlineCollection = nil
    }

    func openLink(_ text: String) {
        onlineCollection = nil
        onlineWork({ try $0.openOnlineLink(text: text) }) { [self] collection in
            onlineSongs = collection.songs
            onlineReleases = collection.releases
            onlineArtists = collection.artists
            onlineCollection = collection
            if collection.songs.isEmpty, collection.releases.isEmpty, collection.artists.isEmpty {
                library.notice("No music could be read from this link")
            }
        }
    }

    func saveWanted(_ songs: [SearchSong]) {
        work { [weak self] native in
            let count = try await Work.aside {
                stride(from: 0, to: songs.count, by: Features.wantedChunk).reduce(UInt64(0)) { total, start in
                    total + ((try? native.saveWanted(songs: Array(songs[start..<min(start + Features.wantedChunk, songs.count)]))) ?? 0)
                }
            }
            await self?.refreshWanted(native)
            self?.library.notice("Saved \(count) recordings for later")
        }
    }

    func loadWanted() {
        work { [weak self] native in await self?.refreshWanted(native) }
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
        } catch {
            library.report(error)
        }
    }

    func importPlaylist(_ name: String, _ songs: [SearchSong]) {
        work { [weak self] native in
            let (imported, found) = try await Work.inOrder {
                (try native.importMatchedPlaylist(name: name, songs: songs), try native.playlists())
            }
            await MainActor.run {
                self?.playlists = found
                self?.library.notice("Imported \(imported.matched) local songs; \(imported.missing) still missing")
            }
        }
    }

    func artistAlbums(_ id: String, from: UInt32 = 0) {
        onlineWork({ try $0.artistAlbums(id: id, from: from) }) { [self] page in
            if from == 0 {
                onlineAlbums = page.albums
            } else {
                var seen = Set(onlineAlbums.map(\.id))
                onlineAlbums += page.albums.filter { seen.insert($0.id).inserted }
            }
            artistAlbumsNext = page.next
        }
    }

    func searchAlbums(_ text: String) {
        clearCollection()
        onlineWork({ try $0.searchAlbumsOnline(text: text) }) { [self] found in
            onlineAlbums = found
            artistAlbumsNext = nil
        }
    }

    func searchArtists(_ text: String) {
        clearCollection()
        onlineWork({ try $0.searchArtistsOnline(text: text) }) { [self] found in onlineArtists = found }
    }

    func loadReleases(_ group: String) {
        onlineWork({ try $0.onlineReleases(group: group) }) { [self] found in onlineReleases = found }
    }

    func releaseSongs(_ id: String) {
        onlineWork({ try $0.releaseSongs(id: id) }) { [self] found in onlineSongs = found }
    }

    func searchOnline(_ text: String) {
        clearCollection()
        onlineWork({ try $0.searchOnline(text: text) }) { [self] found in onlineSongs = found }
    }

    private func onlineWork<Value: Sendable>(
        _ action: @escaping @Sendable (MobilePlayer) throws -> Value,
        publish: @escaping @MainActor @Sendable (Value) -> Void
    ) {
        searchRequest += 1
        let request = searchRequest
        searching = true
        work { [weak self] native in
            defer { if request == self?.searchRequest { self?.searching = false } }
            let value = try await Work.aside { try action(native) }
            await MainActor.run { if request == self?.searchRequest { publish(value) } }
        }
    }

    func visualize(_ enabled: Bool) {
        visualization?.cancel()
        visualization = nil
        guard enabled else { return }
        visualization = Task { [weak self] in
            guard let native = try? await self?.library.player() else { return }
            while !Task.isCancelled {
                if let frame = try? await Work.aside({ try native.visualizer(enabled: true) }) {
                    self?.samples = frame
                }
                try? await Task.sleep(for: Features.visualizerFrame)
            }
            _ = try? await Work.aside { try native.visualizer(enabled: false) }
        }
    }
}
