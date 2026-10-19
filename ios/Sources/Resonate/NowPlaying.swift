import AVFoundation
import MediaPlayer
import ResonateNative
import Synchronization
import UIKit

@MainActor
final class NowPlaying {
    private static let scaleWithoutAScreen: CGFloat = 3
    private static let secondsPerMillisecond = 0.001
    private static let driftBeforeRewriteMs: Double = 1500
    private static let rateSettles: [Duration] = [.milliseconds(250), .milliseconds(750), .seconds(2)]
    static let followsSongRateKey = "follow-song-rate"
    static let playbackSettings = UserDefaults(suiteName: "playback") ?? .standard

    static var followsSongRate: Bool {
        playbackSettings.object(forKey: followsSongRateKey) == nil || playbackSettings.bool(forKey: followsSongRateKey)
    }

    private struct Shown: Equatable {
        let track: Track
        let transport: Transport
        let durationMs: UInt64
        let queueIndex: Int
        let queueCount: Int
        let shuffle: Bool
        let repeatMode: UInt32
        let artRevision: UInt64
        let artworkReady: Bool
    }

    private struct Written {
        let positionMs: UInt64
        let at: ContinuousClock.Instant
        let rate: Double

        func expected(at now: ContinuousClock.Instant) -> Double {
            let elapsed = now - at
            let elapsedMs = Double(elapsed.components.seconds) * 1000 + Double(elapsed.components.attoseconds) * 1e-15
            return Double(positionMs) + elapsedMs * rate
        }
    }

    private unowned let library: Library
    private var native: MobilePlayer?
    private var resumeAfterInterruption = false
    private var artworkFor: (track: UInt64, revision: UInt64)?
    private var artwork: MPMediaItemArtwork?
    private var lockScreen: LockScreenArtwork?
    private var artworkTrack: Track?
    private var shown: Shown?
    private var written: Written?
    private var observers: [NSObjectProtocol] = []
    private var ratedTrack: UInt64?
    private var listening = false
    private var reportedRate: UInt32?
    private var settling: Task<Void, Never>?
    private var routeAsks = 0
    private var rebindOwed = false
    private var lastTransport: Transport?
    private var wasSleeping = false

    private struct Activation: Sendable {
        let refusal: (any Swift.Error)?
        let rateAsked: Bool
    }

    init(library: Library) {
        self.library = library
    }

    func start(_ native: MobilePlayer) {
        self.native = native
        configureThenReport(rebind: false)
        let center = NotificationCenter.default
        let session = AVAudioSession.sharedInstance()
        observers = [
            center.addObserver(forName: AVAudioSession.interruptionNotification, object: session, queue: .main) {
                [weak self] note in
                let type = (note.userInfo?[AVAudioSessionInterruptionTypeKey] as? UInt)
                    .flatMap(AVAudioSession.InterruptionType.init(rawValue:))
                let options = (note.userInfo?[AVAudioSessionInterruptionOptionKey] as? UInt)
                    .map(AVAudioSession.InterruptionOptions.init(rawValue:))
                MainActor.assumeIsolated { self?.interrupted(type, options) }
            },
            center.addObserver(forName: AVAudioSession.routeChangeNotification, object: session, queue: .main) {
                [weak self] note in
                let reason = (note.userInfo?[AVAudioSessionRouteChangeReasonKey] as? UInt)
                    .flatMap(AVAudioSession.RouteChangeReason.init(rawValue:))
                MainActor.assumeIsolated { self?.routeChanged(reason) }
            },
            center.addObserver(forName: AVAudioSession.mediaServicesWereResetNotification, object: session, queue: .main) {
                [weak self] _ in
                MainActor.assumeIsolated { self?.configureThenReport(rebind: true) }
            },
        ]
        registerCommands()
        enableCommands(for: nil)
    }

    func activate(loading track: Track? = nil) {
        let wanted = Self.wantedRate(of: track ?? shown?.track)
        Work.enqueueInOrder { [weak self] in
            let activation = Self.activated(preferring: wanted)
            Task { @MainActor in self?.settle(after: activation) }
        }
    }

    private func settle(after activation: Activation) {
        if let refusal = activation.refusal { library.report(refusal) }
        if activation.rateAsked { settleRoute() }
        reportRoute(rebind: false)
    }

    nonisolated private static func activated(preferring wanted: Double) -> Activation {
        let session = AVAudioSession.sharedInstance()
        var refusal: (any Swift.Error)?
        do {
            try session.setActive(true)
        } catch {
            refusal = error
        }
        return Activation(refusal: refusal, rateAsked: preferred(wanted, by: session))
    }

    nonisolated private static func preferred(_ wanted: Double, by session: AVAudioSession) -> Bool {
        guard wanted != session.preferredSampleRate else { return false }
        try? session.setPreferredSampleRate(wanted)
        return true
    }

    private static func wantedRate(of track: Track?) -> Double {
        followsSongRate ? Double(track?.rate ?? 0) : 0
    }

    func beginListening() async throws {
        listening = true
        do {
            try await Work.inOrder {
                let session = AVAudioSession.sharedInstance()
                do {
                    try session.setCategory(.playAndRecord, mode: .measurement, options: [.defaultToSpeaker, .allowBluetoothA2DP])
                    try session.setActive(true)
                } catch {
                    Self.configureSession()
                    throw error
                }
            }
        } catch {
            listening = false
            throw error
        }
    }

    func endListening() {
        listening = false
        guard let native else { return }
        Work.enqueueInOrder { [weak self] in
            Self.configureSession()
            if let now = try? native.transport(), now != .playing, now != .loading {
                try? AVAudioSession.sharedInstance().setActive(false, options: .notifyOthersOnDeactivation)
            }
            Task { @MainActor in self?.reportRoute(rebind: false) }
        }
    }

    func songRateFollowingChanged() {
        askRate(of: shown?.track)
    }

    private func askRate(of track: Track?) {
        let wanted = Self.wantedRate(of: track)
        Task {
            let asked = await Work.inSession { Self.preferred(wanted, by: .sharedInstance()) }
            if asked { settleRoute() }
            reportRoute(rebind: false)
        }
    }

    private func settleRoute() {
        settling?.cancel()
        settling = Task { [weak self] in
            for wait in Self.rateSettles {
                try? await Task.sleep(for: wait)
                let rate = await Work.inSession { UInt32(AVAudioSession.sharedInstance().sampleRate.rounded()) }
                guard !Task.isCancelled, let self else { return }
                if rate != reportedRate { reportRoute(rebind: false) }
            }
        }
    }

    private func configureThenReport(rebind: Bool) {
        Work.enqueueInOrder { [weak self] in
            Self.configureSession()
            Task { @MainActor in self?.reportRoute(rebind: rebind) }
        }
    }

    nonisolated private static func configureSession() {
        try? AVAudioSession.sharedInstance().setCategory(.playback, mode: .default, policy: .longFormAudio)
    }

    private func reportRoute(rebind: Bool) {
        guard native != nil else { return }
        routeAsks += 1
        rebindOwed = rebindOwed || rebind
        let ask = routeAsks
        Task {
            let reading = await Work.inSession { RouteReading.current }
            guard ask == routeAsks else { return }
            report(reading, rebind: rebindOwed)
            rebindOwed = false
        }
    }

    private func report(_ reading: RouteReading, rebind: Bool) {
        if reading != library.routeReading { library.routeReading = reading }
        library.outputRouteChanged(reading.outputs.first.map { OutputRoute(id: $0.id, name: $0.name) })
        let rate = UInt32(reading.sampleRate.rounded())
        let latency = reading.latencyMs
        reportedRate = rate
        library.request { try $0.outputRouteChanged(rate: rate, latencyMs: latency, rebind: rebind) }
    }

    private func interrupted(_ type: AVAudioSession.InterruptionType?, _ options: AVAudioSession.InterruptionOptions?) {
        switch type {
        case .began:
            resumeAfterInterruption = library.transport == .playing || library.transport == .loading
            library.pause()
        case .ended:
            if resumeAfterInterruption, options?.contains(.shouldResume) == true {
                library.resume()
            }
            resumeAfterInterruption = false
        default:
            break
        }
    }

    private func routeChanged(_ reason: AVAudioSession.RouteChangeReason?) {
        if reason == .oldDeviceUnavailable {
            resumeAfterInterruption = false
            library.pause()
        }
        guard !listening else { return }
        reportRoute(rebind: false)
    }

    private func whenLoaded(_ action: (Library) -> Void) -> MPRemoteCommandHandlerStatus {
        guard library.loaded else { return .noSuchContent }
        action(library)
        return .success
    }

    private func registerCommands() {
        let commands = MPRemoteCommandCenter.shared()
        commands.playCommand.addTarget { [weak self] _ in
            MainActor.assumeIsolated { self?.whenLoaded { $0.resume() } ?? .commandFailed }
        }
        commands.pauseCommand.addTarget { [weak self] _ in
            MainActor.assumeIsolated { self?.whenLoaded { $0.pause() } ?? .commandFailed }
        }
        commands.togglePlayPauseCommand.addTarget { [weak self] _ in
            MainActor.assumeIsolated { self?.whenLoaded { $0.toggle() } ?? .commandFailed }
        }
        commands.stopCommand.addTarget { [weak self] _ in
            MainActor.assumeIsolated { self?.whenLoaded { $0.stop() } ?? .commandFailed }
        }
        commands.nextTrackCommand.addTarget { [weak self] _ in
            MainActor.assumeIsolated { self?.whenLoaded { $0.next() } ?? .commandFailed }
        }
        commands.previousTrackCommand.addTarget { [weak self] _ in
            MainActor.assumeIsolated { self?.whenLoaded { $0.previous() } ?? .commandFailed }
        }
        commands.changePlaybackPositionCommand.addTarget { [weak self] event in
            guard let event = event as? MPChangePlaybackPositionCommandEvent else { return .commandFailed }
            let position = UInt64(max(event.positionTime, 0) * 1000)
            return MainActor.assumeIsolated { self?.whenLoaded { $0.seek(to: position) } ?? .commandFailed }
        }
        commands.changeShuffleModeCommand.addTarget { [weak self] event in
            guard let event = event as? MPChangeShuffleModeCommandEvent else { return .commandFailed }
            let shuffled = event.shuffleType != .off
            return MainActor.assumeIsolated { self?.whenLoaded { $0.setShuffle(shuffled) } ?? .commandFailed }
        }
        commands.changeRepeatModeCommand.addTarget { [weak self] event in
            guard let event = event as? MPChangeRepeatModeCommandEvent else { return .commandFailed }
            let mode = Self.repeatMode(of: event.repeatType)
            return MainActor.assumeIsolated { self?.whenLoaded { $0.setRepeat(mode) } ?? .commandFailed }
        }
        commands.likeCommand.addTarget { [weak self] _ in
            MainActor.assumeIsolated {
                guard let library = self?.library, let track = library.snapshot?.current else { return .noSuchContent }
                library.setFavourite(track, !library.isFavourite(track))
                return .success
            }
        }
    }

    nonisolated private static func repeatMode(of type: MPRepeatType) -> RepeatMode {
        switch type {
        case .one: .one
        case .all: .all
        default: .off
        }
    }

    private static func repeatType(of mode: RepeatMode) -> MPRepeatType {
        switch mode {
        case .off: .off
        case .all: .all
        case .one: .one
        }
    }

    private func enableCommands(for shown: Shown?) {
        let commands = MPRemoteCommandCenter.shared()
        let loaded = shown != nil
        let lastRow = (shown?.queueIndex ?? 0) + 1 >= (shown?.queueCount ?? 0)
        let wraps = shown.map { RepeatMode(engineValue: $0.repeatMode) == .all } ?? false

        for command in [
            commands.playCommand, commands.pauseCommand, commands.togglePlayPauseCommand, commands.stopCommand,
            commands.previousTrackCommand, commands.changePlaybackPositionCommand, commands.changeShuffleModeCommand,
            commands.changeRepeatModeCommand, commands.likeCommand,
        ] {
            command.isEnabled = loaded
        }
        commands.nextTrackCommand.isEnabled = loaded && (!lastRow || wraps)
        commands.likeCommand.isActive = shown?.track.favourite ?? false
        commands.likeCommand.localizedTitle = shown?.track.favourite == true ? "Remove from favourites" : "Add to favourites"
        commands.changeShuffleModeCommand.currentShuffleType = shown?.shuffle == true ? .items : .off
        commands.changeRepeatModeCommand.currentRepeatType = Self.repeatType(of: RepeatMode(engineValue: shown?.repeatMode ?? 0))
    }

    func favouriteChanged() {
        rewrite()
    }

    func queueChanged() {
        guard let shown, shown.queueCount != library.queue.count else { return }
        rewrite()
    }

    private func rewrite() {
        guard native != nil, var state = library.snapshot else { return }
        state.positionMs = library.clock.positionMs
        state.durationMs = library.clock.durationMs
        update(state, forced: true)
    }

    private func releaseSessionIfDone(_ snapshot: Snapshot) {
        let sleeping = snapshot.sleepRemainingMs != nil
        let stopped = snapshot.transport == .stopped && lastTransport != nil && lastTransport != .stopped
        let sleptOff = snapshot.transport == .paused && lastTransport == .playing && wasSleeping && !sleeping
        lastTransport = snapshot.transport
        wasSleeping = sleeping
        guard (stopped || sleptOff), !listening, let native else { return }
        Work.enqueueInOrder {
            guard let now = try? native.transport(), now != .playing, now != .loading else { return }
            try? AVAudioSession.sharedInstance().setActive(false, options: .notifyOthersOnDeactivation)
        }
    }

    func update(_ snapshot: Snapshot, forced: Bool = false) {
        releaseSessionIfDone(snapshot)
        let center = MPNowPlayingInfoCenter.default()
        guard let track = snapshot.current else {
            guard shown != nil || center.nowPlayingInfo != nil else { return }
            center.nowPlayingInfo = nil
            center.playbackState = .stopped
            shown = nil
            ratedTrack = nil
            written = nil
            artworkFor = nil
            artwork = nil
            lockScreen = nil
            artworkTrack = nil
            enableCommands(for: nil)
            return
        }
        if artworkFor?.track != track.id || artworkFor?.revision != track.artRevision {
            artworkFor = (track.id, track.artRevision)
            artworkTrack = track
            artwork = nil
            lockScreen = nil
            loadArtwork(track)
        }
        let next = Shown(
            track: track,
            transport: snapshot.transport,
            durationMs: snapshot.durationMs,
            queueIndex: Int(snapshot.queueIndex),
            queueCount: library.queue.count,
            shuffle: snapshot.shuffle,
            repeatMode: snapshot.repeat,
            artRevision: track.artRevision,
            artworkReady: artwork != nil
        )
        let now = ContinuousClock.now
        let drift = written.map { abs($0.expected(at: now) - Double(snapshot.positionMs)) } ?? .infinity

        if track.id != ratedTrack {
            ratedTrack = track.id
            askRate(of: track)
        }
        guard forced || next != shown || drift > Self.driftBeforeRewriteMs else { return }
        write(next, position: snapshot.positionMs, at: now)
    }

    private func write(_ next: Shown, position: UInt64, at now: ContinuousClock.Instant) {
        let track = next.track
        let rate = next.transport == .playing ? 1.0 : 0.0
        var info: [String: Any] = [
            MPMediaItemPropertyTitle: track.shownTitle,
            MPMediaItemPropertyArtist: track.artist,
            MPMediaItemPropertyAlbumTitle: track.album,
            MPMediaItemPropertyAlbumArtist: track.albumArtist,
            MPMediaItemPropertyPlaybackDuration: Double(next.durationMs) * Self.secondsPerMillisecond,
            MPNowPlayingInfoPropertyElapsedPlaybackTime: Double(position) * Self.secondsPerMillisecond,
            MPNowPlayingInfoPropertyPlaybackRate: rate,
            MPNowPlayingInfoPropertyDefaultPlaybackRate: 1.0,
            MPNowPlayingInfoPropertyPlaybackQueueIndex: next.queueIndex,
            MPNowPlayingInfoPropertyPlaybackQueueCount: next.queueCount,
            MPNowPlayingInfoPropertyMediaType: MPNowPlayingInfoMediaType.audio.rawValue,
        ]
        if let artwork { info[MPMediaItemPropertyArtwork] = artwork }

        if next.track.favourite != shown?.track.favourite || next.queueIndex != shown?.queueIndex
            || next.queueCount != shown?.queueCount || next.shuffle != shown?.shuffle
            || next.repeatMode != shown?.repeatMode || shown == nil
        {
            enableCommands(for: next)
        }
        let center = MPNowPlayingInfoCenter.default()
        center.nowPlayingInfo = info
        center.playbackState =
            switch next.transport {
            case .playing, .loading: .playing
            case .paused: .paused
            case .stopped: .stopped
            }
        shown = next
        written = Written(positionMs: position, at: now, rate: rate)
    }

    private func loadArtwork(_ track: Track) {
        let id = track.id
        let revision = track.artRevision
        let scale = Self.screenScale
        let asking: @MainActor (ArtworkBucket) -> Void = { [weak self] bucket in self?.fetchArtwork(bucket) }
        Task {
            guard let image = await library.features.artwork(track, bucket: .card) else { return }
            guard artworkFor?.track == id, artworkFor?.revision == revision else { return }
            let held = Self.lockScreenArtwork(image, scale: scale, asking: asking)
            lockScreen = held
            artwork = Self.artwork(held)
            rewrite()
        }
    }

    private func fetchArtwork(_ bucket: ArtworkBucket) {
        guard let track = artworkTrack, let held = lockScreen else { return }
        let id = track.id
        let revision = track.artRevision
        let features = library.features
        Task {
            let image =
                bucket.rawValue > ArtworkBucket.card.rawValue
                ? await features.unkeptArtwork(track, bucket: bucket)
                : await features.artwork(track, bucket: bucket)
            guard let image, artworkFor?.track == id, artworkFor?.revision == revision, lockScreen === held else { return }
            held.keep(image, for: bucket)
            artwork = Self.artwork(held)
            rewrite()
        }
    }

    private static var screenScale: CGFloat {
        UIApplication.shared.connectedScenes.lazy.compactMap { ($0 as? UIWindowScene)?.screen.scale }.first
            ?? scaleWithoutAScreen
    }

    nonisolated private static func lockScreenArtwork(
        _ card: UIImage,
        scale: CGFloat,
        asking: @escaping @MainActor (ArtworkBucket) -> Void
    ) -> LockScreenArtwork {
        LockScreenArtwork(first: card, bucket: .card, scale: scale) { @Sendable bucket in
            Task { @MainActor in asking(bucket) }
        }
    }

    nonisolated private static func artwork(_ held: LockScreenArtwork) -> MPMediaItemArtwork {
        MPMediaItemArtwork(boundsSize: held.bounds) { @Sendable size in held.image(for: size) }
    }
}

final class LockScreenArtwork: Sendable {
    private struct Held {
        var images: [ArtworkBucket: UIImage]
        var asked: Set<ArtworkBucket>
    }

    let bounds: CGSize
    private let first: UIImage
    private let scale: CGFloat
    private let held: Mutex<Held>
    private let wanted: @Sendable (ArtworkBucket) -> Void

    init(first: UIImage, bucket: ArtworkBucket, scale: CGFloat, wanted: @escaping @Sendable (ArtworkBucket) -> Void) {
        let largest = CGFloat(ArtworkBucket.full.rawValue)
        let edge = max(first.size.width, first.size.height, 1)
        bounds = CGSize(width: first.size.width * largest / edge, height: first.size.height * largest / edge)
        self.first = first
        self.scale = scale
        self.wanted = wanted
        held = Mutex(Held(images: [bucket: first], asked: [bucket]))
    }

    func image(for size: CGSize) -> UIImage {
        let bucket = ArtworkBucket.fitting(points: max(size.width, size.height), scale: scale)
        let (image, ask) = held.withLock { held -> (UIImage?, Bool) in
            if let exact = held.images[bucket] { return (exact, false) }
            return (Self.nearest(bucket, in: held.images), held.asked.insert(bucket).inserted)
        }
        if ask { wanted(bucket) }
        return image ?? first
    }

    func keep(_ image: UIImage, for bucket: ArtworkBucket) {
        held.withLock { $0.images[bucket] = image }
    }

    private static func nearest(_ bucket: ArtworkBucket, in images: [ArtworkBucket: UIImage]) -> UIImage? {
        let larger = images.filter { $0.key.rawValue >= bucket.rawValue }.min { $0.key.rawValue < $1.key.rawValue }
        return larger?.value ?? images.max { $0.key.rawValue < $1.key.rawValue }?.value
    }
}
