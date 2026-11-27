import AVFoundation
import MediaPlayer
import os
import ResonateLogic
import ResonateNative
import Synchronization
import UIKit

@MainActor
final class NowPlaying {
    private static let scaleWithoutAScreen: CGFloat = 3
    private static let secondsPerMillisecond = 0.001
    private static let rateSettles: [Duration] = [.milliseconds(250), .milliseconds(750), .seconds(2)]
    nonisolated private static let log = Logger(subsystem: "org.resonate.ios", category: "session")
    static let followsSongRateKey = "follow-song-rate"
    static let playbackSettings = UserDefaults(suiteName: "playback") ?? .standard

    static var followsSongRate: Bool {
        playbackSettings.object(forKey: followsSongRateKey) == nil || playbackSettings.bool(forKey: followsSongRateKey)
    }

    private typealias Shown = NowPlayingShown

    private unowned let library: Library
    private var native: MobilePlayer?
    private var interruptions = InterruptionPolicy()
    private var artworkFor: (track: String, revision: UInt64)?
    private var artwork: MPMediaItemArtwork?
    private var lockScreen: LockScreenArtwork?
    private var artworkTrack: Track?
    private var shown: Shown?
    private var heardLast: HeardElsewhere?
    private var written: WrittenPosition?
    private var observers: [NSObjectProtocol] = []
    private var ratedTrack: UInt64?
    private var listening = false
    private var reportedRate: UInt32?
    private var settling: Task<Void, Never>?
    private var routeAsks = 0
    private var rebindOwed = false
    private var release = SessionRelease()
    private var sessionAsked = SessionAsked()

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
                MainActor.assumeIsolated { self?.servicesReset() }
            },
        ]
        registerCommands()
        enableCommands(for: nil)
    }

    func activation(loading track: Track? = nil) -> SessionActivation {
        sessionAsked.ask()
        return SessionActivation(wanted: Self.wantedRate(of: track ?? shown?.track)) { @Sendable [weak self] activation in
            Task { @MainActor in self?.settle(after: activation) }
        }
    }

    private func activate() {
        let session = activation()
        Work.enqueueInOrder { _ = session.held() }
    }

    private func settle(after activation: SessionActivated) {
        guard activation.refusal == nil else { return }
        if activation.rateAsked { settleRoute() }
        reportRoute(rebind: false)
        perform(interruptions.listenerSounded())
    }

    func refusedToSound(_ zone: UInt32?) {
        guard let zone else { return }
        interruptions.refusedToSound([zone])
    }

    func listenerPaused() {
        interruptions.listenerPaused()
    }

    func cameForward() {
        perform(interruptions.cameForward())
    }

    private func servicesReset() {
        configureThenReport(rebind: true)
        perform(interruptions.servicesReset())
    }

    private func resume(_ zones: [UInt32]) {
        guard native != nil else {
            interruptions.resumeRefused()
            return
        }
        sessionAsked.ask()
        let wanted = Self.wantedRate(of: shown?.track)
        Work.enqueueInOrder { [weak self] in
            let activation = Self.activated(preferring: wanted)
            Task { @MainActor in self?.resume(zones, after: activation) }
        }
    }

    private func resume(_ zones: [UInt32], after activation: SessionActivated) {
        guard activation.refusal == nil else {
            interruptions.resumeRefused()
            return
        }
        guard interruptions.resumed() else { return }
        if activation.rateAsked { settleRoute() }
        Task {
            let reading = await Work.inSession { RouteReading.current }
            report(reading, rebind: true)
            library.playZones(zones)
        }
    }

    nonisolated static func activated(preferring wanted: Double) -> SessionActivated {
        let session = AVAudioSession.sharedInstance()
        do {
            try session.setActive(true)
        } catch {
            log.warning("the audio session would not activate: \(error.localizedDescription, privacy: .public)")
            return SessionActivated(refusal: error, rateAsked: preferred(wanted, by: session))
        }
        return SessionActivated(refusal: nil, rateAsked: preferred(wanted, by: session))
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
        let native = native
        Work.enqueueInOrder { [weak self] in
            Self.configureSession()
            if let state = try? native?.snapshot(), !state.anyZoneSounds {
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
        let heard: SessionInterruption? =
            switch type {
            case .began: .began
            case .ended: .ended(shouldResume: options?.contains(.shouldResume) == true)
            default: nil
            }
        perform(interruptions.heard(heard))
    }

    private func perform(_ action: SessionAction) {
        switch action {
        case .pauseSounding: library.pauseSoundingZones { [weak self] paused in self?.interruptions.paused(paused) }
        case .resume(let zones): resume(zones)
        case .nothing: break
        }
    }

    private func routeChanged(_ reason: AVAudioSession.RouteChangeReason?) {
        if reason == .oldDeviceUnavailable { perform(interruptions.lostTheDevice()) }
        guard !listening else { return }
        reportRoute(rebind: false)
    }

    private func whenLoaded(_ action: (Library) -> Void) -> MPRemoteCommandHandlerStatus {
        guard library.loaded else { return .noSuchContent }
        action(library)
        return .success
    }

    private func answer(_ command: LockScreenCommand) -> MPRemoteCommandHandlerStatus {
        let speaksFor = shown?.speaksFor ?? .shown
        return whenLoaded { library in
            switch ZonesHeard.answering(command, speaksFor: speaksFor) {
            case .nothing: break
            case .resumeShown: library.resume()
            case .pauseShown: library.pause()
            case .toggleShown: library.toggle()
            case .pauseSounding:
                listenerPaused()
                library.pauseSoundingZones()
            }
        }
    }

    private func registerCommands() {
        let commands = MPRemoteCommandCenter.shared()
        commands.playCommand.addTarget { [weak self] _ in
            MainActor.assumeIsolated { self?.answer(.play) ?? .commandFailed }
        }
        commands.pauseCommand.addTarget { [weak self] _ in
            MainActor.assumeIsolated { self?.answer(.pause) ?? .commandFailed }
        }
        commands.togglePlayPauseCommand.addTarget { [weak self] _ in
            MainActor.assumeIsolated { self?.answer(.toggle) ?? .commandFailed }
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
        let available = CommandAvailability(shown)

        for command in [commands.playCommand, commands.pauseCommand, commands.togglePlayPauseCommand] {
            command.isEnabled = available.loaded
        }
        for command in [
            commands.stopCommand, commands.previousTrackCommand, commands.changePlaybackPositionCommand,
            commands.changeShuffleModeCommand, commands.changeRepeatModeCommand, commands.likeCommand,
        ] {
            command.isEnabled = available.steers
        }
        commands.nextTrackCommand.isEnabled = available.next
        commands.likeCommand.isActive = shown?.track.favourite ?? false
        commands.likeCommand.localizedTitle = said(shown?.track.favourite == true ? "favourite.remove" : "favourite.add")
        commands.changeShuffleModeCommand.currentShuffleType = shown?.shuffle == true ? .items : .off
        commands.changeRepeatModeCommand.currentRepeatType = Self.repeatType(of: RepeatMode(engineValue: shown?.repeatMode ?? 0))
    }

    func favouriteChanged() {
        rewrite()
    }

    func queueChanged() {
        guard let shown, shown.speaksFor == .shown, shown.queueCount != library.queue.count else { return }
        rewrite()
    }

    private func rewrite() {
        guard native != nil, var state = library.snapshot else { return }
        state.positionMs = library.clock.positionMs
        state.durationMs = library.clock.durationMs
        update(state, forced: true)
    }

    private func releaseSessionIfDone(_ snapshot: Snapshot) {
        guard release.owed(by: snapshot, listening: listening), let native else { return }
        Work.enqueueInOrder {
            guard let state = try? native.snapshot(), !state.anyZoneSounds else { return }
            try? AVAudioSession.sharedInstance().setActive(false, options: .notifyOthersOnDeactivation)
        }
    }

    func update(_ given: Snapshot, answered: Snapshot? = nil, forced: Bool = false) {
        var snapshot = given
        if answered == nil {
            snapshot.heardElsewhere = heardLast
        } else {
            heardLast = snapshot.heardElsewhere
        }
        if let answered {
            releaseSessionIfDone(answered)
            if sessionAsked.owed(by: answered), !listening { activate() }
        }
        let center = MPNowPlayingInfoCenter.default()
        guard let spoken = NowPlayingSpoken(snapshot) else {
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
        let track = spoken.track
        let elsewhere = spoken.zoneHeard != nil
        if artworkFor?.track != track.pictureKey || artworkFor?.revision != track.artRevision {
            artworkFor = (track.pictureKey, track.artRevision)
            artworkTrack = track
            artwork = nil
            lockScreen = nil
            loadArtwork(track)
        }
        let next = Shown(
            track: track,
            transport: spoken.transport,
            durationMs: spoken.durationMs,
            queueIndex: elsewhere ? 0 : Int(snapshot.queueIndex),
            queueCount: elsewhere ? 1 : library.queue.count,
            shuffle: !elsewhere && snapshot.shuffle,
            repeatMode: elsewhere ? 0 : snapshot.repeat,
            artRevision: track.artRevision,
            artworkReady: artwork != nil,
            zoneHeard: spoken.zoneHeard
        )
        let now = ContinuousClock.now
        let drift = written.map { abs($0.expected(at: now) - Double(spoken.positionMs)) } ?? .infinity

        if track.id != ratedTrack {
            ratedTrack = track.id
            askRate(of: track)
        }
        guard next.rewrites(over: shown, forced: forced, drift: drift) else { return }
        write(next, position: spoken.positionMs, at: now)
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

        if next.commandsChanged(from: shown) {
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
        written = WrittenPosition(positionMs: position, at: now, rate: rate)
    }

    private func loadArtwork(_ track: Track) {
        let id = track.pictureKey
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
        let id = track.pictureKey
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

struct SessionActivated: Sendable {
    let refusal: (any Swift.Error)?
    let rateAsked: Bool
}

struct SessionRefused: Swift.Error {
    let zone: UInt32?
}

struct SessionActivation: Sendable {
    let wanted: Double
    let settled: @Sendable (SessionActivated) -> Void

    func held(for native: MobilePlayer) throws {
        guard held() else { throw SessionRefused(zone: (try? native.snapshot())?.zone) }
    }

    func held() -> Bool {
        let activation = NowPlaying.activated(preferring: wanted)
        settled(activation)
        return activation.refusal == nil
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
