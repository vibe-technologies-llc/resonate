import AVFoundation
import Foundation
import Observation
import ResonateLogic
import ResonateNative

@MainActor
@Observable
final class Listening {
    static let clipSeconds: TimeInterval = 12
    nonisolated static let triesOnAMiss = ListenNext.triesOnAMiss
    static let followedEverySeconds = 20
    private static let heardKept = 8

    private enum Outcome: Sendable {
        case answered(HeardAnswer)
        case denied
        case failed(String)
        case stopped
    }

    @ObservationIgnored private unowned let library: Library
    @ObservationIgnored private let accounts = AccountStore(secrets: Keychain())
    @ObservationIgnored private var running: Task<Void, Never>?
    @ObservationIgnored private var tokenRead: Task<Void, Never>?

    private(set) var stage = ListenStage.idle
    private(set) var heard: [HeardSong] = []
    private(set) var following = ListenFollowing.off
    private(set) var keepsListening = false
    private(set) var auddToken: String
    private(set) var named = 0

    init(library: Library) {
        self.library = library
        auddToken = ""
        let store = accounts
        tokenRead = Task { [weak self] in
            guard let token = try? await Work.inSecrets({ try store.readAuddToken() }) else { return }
            self?.auddToken = token
        }
    }

    func listen() {
        guard !stage.isListening else { return }
        let previous = running
        stopFollowing()
        guard library.features.online else {
            stage = .offline
            return
        }
        stage = .recording(nth: 1, since: .now)
        running = Task {
            await previous?.value
            await self.name()
        }
    }

    func stop() {
        running?.cancel()
        following = .off
        if stage.isListening { stage = .idle }
    }

    func keepListening(_ keeps: Bool) {
        keepsListening = keeps
        if !keeps {
            stopFollowing()
        } else if !stage.isListening, following == .off, stage.namedOrMissed {
            let previous = running
            running = Task {
                await previous?.value
                await self.follow()
            }
        }
    }

    func saveAuddToken(_ value: String) {
        let token = value.trimmingCharacters(in: .whitespacesAndNewlines)
        let store = accounts
        Task {
            do {
                try await Work.inSecrets { try store.writeAuddToken(token) }
                auddToken = token
                library.notice(token.isEmpty ? "AudD will no longer be asked" : "AudD will be asked when Shazam does not know a song")
            } catch {
                library.report(error)
            }
        }
    }

    private func stopFollowing() {
        guard following != .off else { return }
        running?.cancel()
        following = .off
    }

    private func name() async {
        var nth = 1
        while true {
            stage = .recording(nth: nth, since: .now)
            let outcome = await recordAndAsk { self.stage = .asking }
            guard !Task.isCancelled else { return }
            switch outcome {
            case .stopped:
                return
            case .denied:
                stage = .denied
                return
            case .failed(let text):
                stage = .failed(text)
                return
            case .answered(let answer):
                switch ListenNext.after(answer, naming: nth, standing: nil) {
                case .tell(let song):
                    land(song)
                case .again(let next):
                    guard library.features.online else {
                        stage = .offline
                        return
                    }
                    nth = next
                    continue
                case .settle(let settled):
                    stage = settled
                case .keep:
                    break
                }
            }
            break
        }
        await follow()
    }

    private func follow() async {
        while keepsListening, stage.namedOrMissed {
            following = .waiting
            guard (try? await Task.sleep(for: .seconds(Self.followedEverySeconds))) != nil, keepsListening else { break }
            guard library.features.online else {
                stage = .offline
                break
            }
            following = .listening
            let outcome = await recordAndAsk {}
            guard !Task.isCancelled else { return }
            switch outcome {
            case .answered(let answer):
                switch answer {
                case .unreached(let service):
                    stage = .unreached(service)
                default:
                    if case .tell(let song) = ListenNext.after(answer, naming: nil, standing: standing) {
                        land(song)
                    }
                }
            case .denied:
                stage = .denied
            case .failed(let text):
                stage = .failed(text)
            case .stopped:
                break
            }
        }
        if !Task.isCancelled { following = .off }
    }

    private var standing: HeardSong? {
        if case .found(let song) = stage { song } else { nil }
    }

    private func recordAndAsk(asking: () -> Void) async -> Outcome {
        guard await Microphone.allowed() else { return .denied }
        if library.transport == .playing || library.transport == .loading { library.pause() }
        let clip: HeardClip
        do {
            clip = try await Microphone.record(for: Self.clipSeconds, through: library.nowPlaying)
        } catch is CancellationError {
            return .stopped
        } catch {
            return .failed(explained(error))
        }
        guard !Task.isCancelled else { return .stopped }
        asking()
        await tokenRead?.value
        let token = auddToken
        let native: MobilePlayer
        do {
            native = try await library.player()
        } catch {
            return .failed(explained(error))
        }
        let answer = OnlyOnce<Outcome>()
        Task.detached {
            do {
                let heard = try await Work.aside {
                    try native.recogniseHeard(samples: clip.samples, rate: clip.rate, auddToken: token)
                }
                answer.give(.answered(heard))
            } catch {
                answer.give(.failed(explained(error)))
            }
        }
        return await withTaskCancellationHandler {
            await answer.value()
        } onCancel: {
            answer.give(.stopped)
        }
    }

    private func land(_ song: HeardSong) {
        heard.removeAll { $0.key == song.key }
        heard.insert(song, at: 0)
        if heard.count > Self.heardKept { heard.removeLast(heard.count - Self.heardKept) }
        stage = .found(song)
        named += 1
    }
}

struct HeardClip: Sendable {
    let samples: [Float]
    let rate: UInt32
}

final class OpenMicrophone: @unchecked Sendable {
    private static let lowestRate: Double = 8000
    private static let tapFrames: AVAudioFrameCount = 4096

    let sink: ClipSink
    let rate: Double
    private let engine: AVAudioEngine
    private let watching: [any NSObjectProtocol]

    init(for seconds: TimeInterval) throws {
        let engine = AVAudioEngine()
        let input = engine.inputNode
        let format = input.outputFormat(forBus: 0)
        guard format.sampleRate >= Self.lowestRate, format.channelCount > 0 else { throw MicrophoneFailure.unavailable }

        let sink = ClipSink(wanted: Int(format.sampleRate * seconds))
        input.installTap(onBus: 0, bufferSize: Self.tapFrames, format: format, block: ClipSink.tap(into: sink))
        self.engine = engine
        self.sink = sink
        rate = format.sampleRate
        watching = sink.interruptions(of: engine)
        engine.prepare()
        do {
            try engine.start()
        } catch {
            close()
            throw error
        }
    }

    func close() {
        watching.forEach { NotificationCenter.default.removeObserver($0) }
        engine.inputNode.removeTap(onBus: 0)
        engine.stop()
    }
}

@MainActor
enum Microphone {
    private static let graceSeconds: Double = 3

    static func allowed() async -> Bool {
        switch AVAudioApplication.shared.recordPermission {
        case .granted: true
        case .denied: false
        default: await AVAudioApplication.requestRecordPermission()
        }
    }

    static func record(for seconds: TimeInterval, through session: NowPlaying) async throws -> HeardClip {
        try await session.beginListening()
        let opened: OpenMicrophone
        do {
            opened = try await Work.inSession { Result { try OpenMicrophone(for: seconds) } }.get()
        } catch {
            session.endListening()
            throw error
        }

        let heard: Result<[Float], any Swift.Error>
        do {
            heard = .success(try await filled(opened, within: seconds + graceSeconds))
        } catch {
            heard = .failure(error)
        }
        await Work.inSession { opened.close() }
        session.endListening()
        return HeardClip(samples: try heard.get(), rate: UInt32(opened.rate.rounded()))
    }

    private static func filled(_ opened: OpenMicrophone, within givenUpAfter: TimeInterval) async throws -> [Float] {
        let sink = opened.sink
        let deadline = Task.detached {
            guard (try? await Task.sleep(for: .seconds(givenUpAfter))) != nil else { return }
            sink.finish()
        }
        defer { deadline.cancel() }
        return try await sink.filled()
    }
}

extension ClipSink {
    nonisolated static func tap(into sink: ClipSink) -> AVAudioNodeTapBlock {
        { buffer, _ in sink.take(buffer) }
    }

    nonisolated func interruptions(of engine: AVAudioEngine) -> [any NSObjectProtocol] {
        let center = NotificationCenter.default
        return [
            center.addObserver(forName: AVAudioSession.interruptionNotification, object: nil, queue: nil) { @Sendable [self] note in
                let began = (note.userInfo?[AVAudioSessionInterruptionTypeKey] as? UInt)
                    .flatMap(AVAudioSession.InterruptionType.init(rawValue:)) == .began
                if began { interrupt() }
            },
            center.addObserver(forName: .AVAudioEngineConfigurationChange, object: engine, queue: nil) { @Sendable [self] _ in
                interrupt()
            },
        ]
    }

    private func take(_ buffer: AVAudioPCMBuffer) {
        guard let channels = buffer.floatChannelData else { return }
        let frames = Int(buffer.frameLength)
        let count = Int(buffer.format.channelCount)
        guard frames > 0, count > 0 else { return }

        let interleaved = buffer.format.isInterleaved
        let stride = interleaved ? count : 1
        var mono = [Float](repeating: 0, count: frames)
        for channel in 0..<count {
            let samples = interleaved ? channels[0] + channel : channels[channel]
            for frame in 0..<frames { mono[frame] += samples[frame * stride] }
        }
        append(mono, sharing: 1 / Float(count))
    }
}
