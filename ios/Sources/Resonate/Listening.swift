import AVFoundation
import Foundation
import Observation
import ResonateNative
import Synchronization

enum ListenStage: Equatable {
    case idle
    case recording(nth: Int, since: Date)
    case asking
    case found(HeardSong)
    case unknown
    case silent
    case unreached(String)
    case offline
    case denied
    case failed(String)

    var isListening: Bool {
        switch self {
        case .recording, .asking: true
        default: false
        }
    }

    var namedOrMissed: Bool {
        switch self {
        case .found, .unknown: true
        default: false
        }
    }
}

enum ListenFollowing: Equatable {
    case off
    case waiting
    case listening
}

enum ListenNext: Equatable {
    case tell(HeardSong)
    case again(nth: Int)
    case settle(ListenStage)
    case keep

    static func after(_ answer: HeardAnswer, naming nth: Int?, standing: HeardSong?) -> ListenNext {
        switch (nth, answer) {
        case (nil, .named(let song)) where standing?.key == song.key: .keep
        case (_, .named(let song)): .tell(song)
        case (nil, _): .keep
        case (let nth?, .missed) where nth < Listening.triesOnAMiss, (let nth?, .silent) where nth < Listening.triesOnAMiss:
            .again(nth: nth + 1)
        case (_?, .missed): .settle(.unknown)
        case (_?, .silent): .settle(.silent)
        case (_?, .unreached(let service)): .settle(.unreached(service))
        }
    }
}

@MainActor
@Observable
final class Listening {
    static let clipSeconds: TimeInterval = 12
    nonisolated static let triesOnAMiss = 3
    static let followedEverySeconds = 20
    private static let heardKept = 8

    private enum Outcome: Sendable {
        case answered(HeardAnswer)
        case denied
        case failed(String)
        case stopped
    }

    @ObservationIgnored private unowned let library: Library
    @ObservationIgnored private let accounts = AccountStore()
    @ObservationIgnored private var running: Task<Void, Never>?

    private(set) var stage = ListenStage.idle
    private(set) var heard: [HeardSong] = []
    private(set) var following = ListenFollowing.off
    private(set) var keepsListening = false
    private(set) var auddToken: String
    private(set) var named = 0

    init(library: Library) {
        self.library = library
        auddToken = (try? accounts.readAuddToken()) ?? ""
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

final class OnlyOnce<Value: Sendable>: Sendable {
    private struct Held: Sendable {
        var done = false
        var early: Value?
        var waiting: CheckedContinuation<Value, Never>?
    }

    private let held = Mutex(Held())

    func give(_ value: Value) {
        let waiting = held.withLock { held -> CheckedContinuation<Value, Never>? in
            guard !held.done else { return nil }
            held.done = true
            guard let waiting = held.waiting else {
                held.early = value
                return nil
            }
            held.waiting = nil
            return waiting
        }
        waiting?.resume(returning: value)
    }

    func value() async -> Value {
        await withCheckedContinuation { continuation in
            let ready = held.withLock { held -> Value? in
                if let early = held.early { return early }
                held.waiting = continuation
                return nil
            }
            if let ready { continuation.resume(returning: ready) }
        }
    }
}

struct HeardClip: Sendable {
    let samples: [Float]
    let rate: UInt32
}

enum MicrophoneFailure: LocalizedError {
    case unavailable
    case interrupted

    var errorDescription: String? {
        switch self {
        case .unavailable: "No microphone is available to listen with."
        case .interrupted: "Listening was interrupted by a call, a change of headphones or another app."
        }
    }
}

@MainActor
enum Microphone {
    private static let lowestRate: Double = 8000
    private static let tapFrames: AVAudioFrameCount = 4096
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
        defer { session.endListening() }

        let engine = AVAudioEngine()
        let input = engine.inputNode
        let format = input.outputFormat(forBus: 0)
        guard format.sampleRate >= lowestRate, format.channelCount > 0 else { throw MicrophoneFailure.unavailable }

        let sink = ClipSink(wanted: Int(format.sampleRate * seconds))
        input.installTap(onBus: 0, bufferSize: tapFrames, format: format, block: ClipSink.tap(into: sink))
        let watching = sink.interruptions(of: engine)
        defer {
            watching.forEach { NotificationCenter.default.removeObserver($0) }
            input.removeTap(onBus: 0)
            engine.stop()
        }
        engine.prepare()
        try engine.start()

        let givenUpAfter = seconds + graceSeconds
        let deadline = Task.detached {
            guard (try? await Task.sleep(for: .seconds(givenUpAfter))) != nil else { return }
            sink.finish()
        }
        defer { deadline.cancel() }
        let samples = try await sink.filled()
        return HeardClip(samples: samples, rate: UInt32(format.sampleRate.rounded()))
    }
}

final class ClipSink: Sendable {
    private struct Held {
        var samples: [Float] = []
        var waiting: CheckedContinuation<[Float], any Swift.Error>?
        var done = false
        var stopped = false
        var interrupted = false
    }

    private let wanted: Int
    private let held: Mutex<Held>

    init(wanted: Int) {
        self.wanted = wanted
        var empty = Held()
        empty.samples.reserveCapacity(wanted)
        held = Mutex(empty)
    }

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

    func interrupt() {
        let waiting = held.withLock { held -> CheckedContinuation<[Float], any Swift.Error>? in
            guard !held.done else { return nil }
            held.done = true
            held.interrupted = true
            defer { held.waiting = nil }
            return held.waiting
        }
        waiting?.resume(throwing: MicrophoneFailure.interrupted)
    }

    func finish() {
        let ready = held.withLock { held -> (CheckedContinuation<[Float], any Swift.Error>, [Float])? in
            guard !held.done else { return nil }
            held.done = true
            defer { held.waiting = nil }
            return held.waiting.map { ($0, held.samples) }
        }
        if let (waiting, samples) = ready { waiting.resume(returning: samples) }
    }

    func filled() async throws -> [Float] {
        try await withTaskCancellationHandler {
            try await withCheckedThrowingContinuation { continuation in
                let ready = held.withLock { held -> Result<[Float], any Swift.Error>? in
                    if held.stopped { return .failure(CancellationError()) }
                    if held.interrupted { return .failure(MicrophoneFailure.interrupted) }
                    if held.done { return .success(held.samples) }
                    held.waiting = continuation
                    return nil
                }
                if let ready { continuation.resume(with: ready) }
            }
        } onCancel: {
            let waiting = held.withLock { held -> CheckedContinuation<[Float], any Swift.Error>? in
                held.stopped = true
                held.done = true
                defer { held.waiting = nil }
                return held.waiting
            }
            waiting?.resume(throwing: CancellationError())
        }
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
        let share = 1 / Float(count)

        let ready = held.withLock { held -> (CheckedContinuation<[Float], any Swift.Error>, [Float])? in
            guard !held.done else { return nil }
            let room = wanted - held.samples.count
            held.samples.append(contentsOf: mono.prefix(room).map { $0 * share })
            guard held.samples.count >= wanted else { return nil }
            held.done = true
            defer { held.waiting = nil }
            return held.waiting.map { ($0, held.samples) }
        }
        if let (waiting, samples) = ready { waiting.resume(returning: samples) }
    }
}
