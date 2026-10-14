import AVFoundation
import MediaPlayer
import ResonateNative
import UIKit

@MainActor
final class NowPlaying {
    nonisolated private static let artworkEdge: CGFloat = 384
    private static let secondsPerMillisecond = 0.001

    private unowned let library: Library
    private var native: MobilePlayer?
    private var resumeAfterInterruption = false
    private var shownTrack: UInt64?
    private var shownRevision: UInt64?
    private var artwork: MPMediaItemArtwork?
    private var observers: [NSObjectProtocol] = []

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
    }

    func activate() {
        try? AVAudioSession.sharedInstance().setActive(true)
        reportRoute(rebind: false)
    }

    private func configureSession() {
        let session = AVAudioSession.sharedInstance()
        try? session.setCategory(.playback, mode: .default, policy: .longFormAudio)
        try? session.setActive(true)
    }

    private func reportRoute(rebind: Bool) {
        guard native != nil else { return }
        let session = AVAudioSession.sharedInstance()
        let rate = UInt32(session.sampleRate.rounded())
        let latency = (session.outputLatency + session.ioBufferDuration) * 1000
        library.request { try $0.outputRouteChanged(rate: rate, latencyMs: latency, rebind: rebind) }
    }

    private func interrupted(_ type: AVAudioSession.InterruptionType?, _ options: AVAudioSession.InterruptionOptions?) {
        switch type {
        case .began:
            resumeAfterInterruption = library.snapshot?.transport == .playing
            library.request { try $0.pause() }
        case .ended:
            if resumeAfterInterruption, options?.contains(.shouldResume) == true {
                activate()
                library.request { try $0.play() }
            }
            resumeAfterInterruption = false
        default:
            break
        }
    }

    private func routeChanged(_ reason: AVAudioSession.RouteChangeReason?) {
        if reason == .oldDeviceUnavailable {
            resumeAfterInterruption = false
            library.request { try $0.pause() }
        }
        reportRoute(rebind: false)
    }

    private func registerCommands() {
        let commands = MPRemoteCommandCenter.shared()
        commands.playCommand.addTarget { [weak self] _ in
            MainActor.assumeIsolated {
                self?.activate()
                self?.library.request { try $0.play() }
            }
            return .success
        }
        commands.pauseCommand.addTarget { [weak self] _ in
            MainActor.assumeIsolated { self?.library.request { try $0.pause() } }
            return .success
        }
        commands.togglePlayPauseCommand.addTarget { [weak self] _ in
            MainActor.assumeIsolated { self?.library.toggle() }
            return .success
        }
        commands.nextTrackCommand.addTarget { [weak self] _ in
            MainActor.assumeIsolated { self?.library.request { try $0.next() } }
            return .success
        }
        commands.previousTrackCommand.addTarget { [weak self] _ in
            MainActor.assumeIsolated { self?.library.request { try $0.previous() } }
            return .success
        }
        commands.changePlaybackPositionCommand.addTarget { [weak self] event in
            guard let event = event as? MPChangePlaybackPositionCommandEvent else { return .commandFailed }
            let position = UInt64(max(event.positionTime, 0) * 1000)
            MainActor.assumeIsolated { self?.library.request { try $0.seek(positionMs: position) } }
            return .success
        }
        commands.likeCommand.addTarget { [weak self] _ in
            MainActor.assumeIsolated {
                guard let self, let track = self.library.snapshot?.current else { return }
                self.library.setFavourite(track, !track.favourite)
            }
            return .success
        }
    }

    func update(_ snapshot: Snapshot) {
        let center = MPNowPlayingInfoCenter.default()
        guard let track = snapshot.current else {
            center.nowPlayingInfo = nil
            center.playbackState = .stopped
            shownTrack = nil
            return
        }
        if track.id != shownTrack || track.artRevision != shownRevision {
            shownTrack = track.id
            shownRevision = track.artRevision
            artwork = nil
            loadArtwork(track)
        }
        MPRemoteCommandCenter.shared().likeCommand.isActive = track.favourite
        var info: [String: Any] = [
            MPMediaItemPropertyTitle: track.title.isEmpty ? track.name : track.title,
            MPMediaItemPropertyArtist: track.artist,
            MPMediaItemPropertyAlbumTitle: track.album,
            MPMediaItemPropertyAlbumArtist: track.albumArtist,
            MPMediaItemPropertyPlaybackDuration: Double(snapshot.durationMs) * Self.secondsPerMillisecond,
            MPNowPlayingInfoPropertyElapsedPlaybackTime: Double(snapshot.positionMs) * Self.secondsPerMillisecond,
            MPNowPlayingInfoPropertyPlaybackRate: snapshot.transport == .playing ? 1.0 : 0.0,
            MPNowPlayingInfoPropertyPlaybackQueueIndex: Int(snapshot.queueIndex),
            MPNowPlayingInfoPropertyPlaybackQueueCount: library.queue.count,
            MPNowPlayingInfoPropertyMediaType: MPNowPlayingInfoMediaType.audio.rawValue,
        ]
        if let artwork { info[MPMediaItemPropertyArtwork] = artwork }
        center.nowPlayingInfo = info
        center.playbackState =
            switch snapshot.transport {
            case .playing, .loading: .playing
            case .paused: .paused
            case .stopped: .stopped
            }
    }

    private func loadArtwork(_ track: Track) {
        let id = track.id
        Task {
            guard let image = await library.features.artwork(track) else { return }
            let thumbnail = await Task.detached(priority: .utility) { Self.thumbnail(of: image) }.value
            guard shownTrack == id else { return }
            artwork = Self.artwork(thumbnail)
            if let state = library.snapshot { update(state) }
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
