import AVFoundation
import Foundation
import ResonateLogic
import ResonateNative

final class OpenSweep: @unchecked Sendable {
    private static let tapFrames: AVAudioFrameCount = 4096

    let sink: ClipSink
    let sweep: MeasurementSweep
    private let engine: AVAudioEngine
    private let player: AVAudioPlayerNode
    private let watching: [any NSObjectProtocol]

    static func inputRate() throws -> UInt32 {
        let format = AVAudioEngine().inputNode.outputFormat(forBus: 0)
        guard format.channelCount > 0, format.sampleRate > 0 else { throw MicrophoneFailure.unavailable }
        return UInt32(format.sampleRate.rounded())
    }

    init(playing sweep: MeasurementSweep) throws {
        let engine = AVAudioEngine()
        let player = AVAudioPlayerNode()
        let input = engine.inputNode
        let heard = input.outputFormat(forBus: 0)
        guard heard.channelCount > 0, UInt32(heard.sampleRate.rounded()) == sweep.rate else {
            throw MicrophoneFailure.unavailable
        }
        let sent = AVAudioFormat(
            commonFormat: .pcmFormatFloat32, sampleRate: Double(sweep.rate), channels: 1, interleaved: false
        )
        guard let sent, let buffer = Self.buffer(holding: sweep.samples, as: sent) else {
            throw MicrophoneFailure.unavailable
        }

        let sink = ClipSink(wanted: Int(sweep.recordedFrames))
        input.installTap(onBus: 0, bufferSize: Self.tapFrames, format: heard, block: ClipSink.tap(into: sink))
        engine.attach(player)
        engine.connect(player, to: engine.mainMixerNode, format: sent)
        self.engine = engine
        self.player = player
        self.sink = sink
        self.sweep = sweep
        watching = sink.interruptions(of: engine)
        engine.prepare()
        do {
            try engine.start()
        } catch {
            close()
            throw error
        }
        player.scheduleBuffer(buffer, at: nil, options: [], completionHandler: nil)
        player.play()
    }

    func close() {
        watching.forEach { NotificationCenter.default.removeObserver($0) }
        player.stop()
        engine.inputNode.removeTap(onBus: 0)
        engine.stop()
    }

    private static func buffer(holding samples: [Float], as format: AVAudioFormat) -> AVAudioPCMBuffer? {
        let frames = AVAudioFrameCount(samples.count)
        guard let buffer = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: frames),
            let channel = buffer.floatChannelData?[0]
        else { return nil }
        samples.withUnsafeBufferPointer { channel.update(from: $0.baseAddress!, count: samples.count) }
        buffer.frameLength = frames
        return buffer
    }
}

extension Microphone {
    private static let tailSeconds: Double = 3

    static func sweep(through session: NowPlaying) async throws -> HeardClip {
        try await session.beginListening()
        let opened: OpenSweep
        do {
            opened = try await Work.inSession {
                Result {
                    let rate = try OpenSweep.inputRate()
                    return try OpenSweep(playing: try measurementSweep(rate: rate))
                }
            }.get()
        } catch {
            session.endListening()
            throw error
        }

        let seconds = Double(opened.sweep.recordedFrames) / Double(opened.sweep.rate)
        let heard: Result<[Float], any Swift.Error>
        do {
            heard = .success(try await filled(opened.sink, within: seconds + tailSeconds))
        } catch {
            heard = .failure(error)
        }
        await Work.inSession { opened.close() }
        session.endListening()
        return HeardClip(samples: try heard.get(), rate: opened.sweep.rate)
    }
}
