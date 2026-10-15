import AVFoundation
import MediaPlayer
import ResonateNative
import UIKit

@MainActor
final class NowPlaying {
    nonisolated private static let artworkEdge: CGFloat = 384
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
    private var shown: Shown?
    private var written: Written?
    private var observers: [NSObjectProtocol] = []
    private var ratedTrack: UInt64?
    private var listening = false
    private var reportedRate: UInt32?
    private var settling: Task<Void, Never>?

    init(library: Library) {
        self.library = library
    }

    func start(_ native: MobilePlayer) {
        self.native = native
        configureSession()
        reportRoute(rebind: false)
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
                MainActor.assumeIsolated {
                    self?.configureSession()
                    self?.reportRoute(rebind: true)
                }
            },
        ]
        registerCommands()
        enableCommands(for: nil)
    }

    func activate() {
        do {
            try AVAudioSession.sharedInstance().setActive(true)
        } catch {
            library.report(error)
        }
        preferRate(of: shown?.track)
        reportRoute(rebind: false)
    }

    func beginListening() throws {
        let session = AVAudioSession.sharedInstance()
        try session.setCategory(.playAndRecord, mode: .measurement, options: [.defaultToSpeaker, .allowBluetoothA2DP])
        try session.setActive(true)
        listening = true
    }

    func endListening() {
        listening = false
        configureSession()
        if library.transport != .playing {
            try? AVAudioSession.sharedInstance().setActive(false, options: .notifyOthersOnDeactivation)
        }
        reportRoute(rebind: false)
    }

    func songRateFollowingChanged() {
        preferRate(of: shown?.track)
        reportRoute(rebind: false)
    }

    private func preferRate(of track: Track?) {
        let session = AVAudioSession.sharedInstance()
        let wanted = Self.followsSongRate ? Double(track?.rate ?? 0) : 0
        guard wanted != session.preferredSampleRate else { return }
        try? session.setPreferredSampleRate(wanted)
        settleRoute()
    }

    private func settleRoute() {
        settling?.cancel()
        settling = Task { [weak self] in
            for wait in Self.rateSettles {
                try? await Task.sleep(for: wait)
                guard !Task.isCancelled, let self else { return }
                let rate = UInt32(AVAudioSession.sharedInstance().sampleRate.rounded())
                if rate != reportedRate { reportRoute(rebind: false) }
            }
        }
    }

    private func configureSession() {
        try? AVAudioSession.sharedInstance().setCategory(.playback, mode: .default, policy: .longFormAudio)
    }

    private func reportRoute(rebind: Bool) {
        guard native != nil else { return }
        let reading = RouteReading.current
        if reading != library.routeReading { library.routeReading = reading }
        library.outputRouteChanged(OutputRoute.current)
        let session = AVAudioSession.sharedInstance()
        let rate = UInt32(session.sampleRate.rounded())
        reportedRate = rate
        let latency = (session.outputLatency + session.ioBufferDuration) * 1000
        library.request { try $0.outputRouteChanged(rate: rate, latencyMs: latency, rebind: rebind) }
    }

    private func interrupted(_ type: AVAudioSession.InterruptionType?, _ options: AVAudioSession.InterruptionOptions?) {
        switch type {
        case .began:
            resumeAfterInterruption = library.transport == .playing
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

    private func rewrite() {
        guard native != nil, var state = library.snapshot else { return }
        state.positionMs = library.clock.positionMs
        state.durationMs = library.clock.durationMs
        update(state, forced: true)
    }

    func update(_ snapshot: Snapshot, forced: Bool = false) {
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
            enableCommands(for: nil)
            return
        }
        if artworkFor?.track != track.id || artworkFor?.revision != track.artRevision {
            artworkFor = (track.id, track.artRevision)
            artwork = nil
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
            preferRate(of: track)
            reportRoute(rebind: false)
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

        let center = MPNowPlayingInfoCenter.default()
        center.nowPlayingInfo = info
        center.playbackState =
            switch next.transport {
            case .playing, .loading: .playing
            case .paused: .paused
            case .stopped: .stopped
            }
        if next.track.favourite != shown?.track.favourite || next.queueIndex != shown?.queueIndex
            || next.queueCount != shown?.queueCount || next.shuffle != shown?.shuffle
            || next.repeatMode != shown?.repeatMode || shown == nil
        {
            enableCommands(for: next)
        }
        shown = next
        written = Written(positionMs: position, at: now, rate: rate)
    }

    private func loadArtwork(_ track: Track) {
        let id = track.id
        let revision = track.artRevision
        Task {
            guard let image = await library.features.artwork(track) else { return }
            let thumbnail = await Task.detached(priority: .utility) { Self.thumbnail(of: image) }.value
            guard artworkFor?.track == id, artworkFor?.revision == revision else { return }
            artwork = Self.artwork(thumbnail)
            rewrite()
        }
    }

    nonisolated private static func thumbnail(of image: UIImage) -> UIImage {
        let scale = min(1, artworkEdge / max(image.size.width, image.size.height, 1))
        let size = CGSize(width: image.size.width * scale, height: image.size.height * scale)
        let format = UIGraphicsImageRendererFormat()
        format.scale = 1

        return UIGraphicsImageRenderer(size: size, format: format).image { _ in
            image.draw(in: CGRect(origin: .zero, size: size))
        }
    }

    nonisolated private static func artwork(_ thumbnail: UIImage) -> MPMediaItemArtwork {
        MPMediaItemArtwork(boundsSize: thumbnail.size) { @Sendable _ in thumbnail }
    }
}
