import Foundation
import ResonateNative

public enum ListeningTesting: CaseIterable, Sendable {
    case equaliser
    case restoration
    case resampling
    case dither

    public var label: String {
        switch self {
        case .equaliser: "Equaliser"
        case .restoration: "Restoration"
        case .resampling: "Resampler"
        case .dither: "Dither"
        }
    }

    public var kind: SettingKind? {
        switch self {
        case .equaliser: nil
        case .restoration: .restoration
        case .resampling: .resampling
        case .dither: .dither
        }
    }

    public var howItWorks: String {
        switch self {
        case .equaliser:
            "A is the equaliser as set, B is the signal with it bypassed. Each is lowered, never raised, until their loudness across 20 Hz to 16 kHz agrees to 0.1 dB, so the louder one cannot give itself away.\(ListeningTestWords.howXIsDrawn) The room correction stays on in both. Editing the curve ends the comparison."
        case .restoration:
            "A is lossy restoration as set, B the setting you choose. Restoration changes how loud a song plays, so the song is first measured through both, which takes a moment, and the louder is lowered, never raised, until the two agree. A lossless file is touched by neither.\(ListeningTestWords.howXIsDrawn)"
        case .resampling:
            "A is the resampler as set, B the quality and phase you choose. Both play at one level. The resampler works only where the song's rate has to be changed to the device's; where it does not, the two sound alike and the test is refused.\(ListeningTestWords.howXIsDrawn)"
        case .dither:
            "A is the dither as the phone plays it, B the kind you choose. Both play at one level. Dither works only where the bits are cut down to the device's depth; where they are not, the two sound alike and the test is refused.\(ListeningTestWords.howXIsDrawn)"
        }
    }
}

public enum ListeningTestWords {
    static let howXIsDrawn =
        " X is one of the two, drawn afresh for every trial: hear A, B and X as often as you like on the song playing, then say which X was. A change is heard a few moments after you press, within the audio already buffered."

    public static let ditherChoices: [TestedDither] = [.off, .flat, .shaped]

    public static func restorationLabel(_ value: LossyRestoration) -> String {
        switch value {
        case .off: "Off"
        case .repair: "Repair"
        case .extend: "Extend"
        }
    }

    public static func qualityLabel(_ value: ResamplingQuality) -> String {
        switch value {
        case .fast: "Fast"
        case .balanced: "Balanced"
        case .high: "High"
        case .veryHigh: "Very high"
        }
    }

    public static func phaseLabel(_ value: ResamplingPhase) -> String {
        switch value {
        case .linear: "Linear"
        case .intermediate: "Intermediate"
        case .minimum: "Minimum"
        }
    }

    public static func ditherLabel(_ value: TestedDither) -> String {
        switch value {
        case .off: "None"
        case .flat: "Triangular"
        case .shaped: "Triangular, shaped"
        }
    }

    public static func playedAs(_ setting: TestedSetting) -> String {
        switch setting {
        case .restoration(let a, _): "A is \(restorationLabel(a))"
        case .resampling(let quality, let phase, _, _): "A is \(qualityLabel(quality)) · \(phaseLabel(phase))"
        case .dither(let a, _): "A is \(ditherLabel(a))"
        }
    }

    public static func beginning(_ testing: ListeningTesting) -> String {
        testing == .restoration ? "Measuring…" : "Starting…"
    }

    public static func levels(_ levels: TestLevels) -> String {
        switch levels {
        case .matched(let residual): "Levels matched to \(String(format: "%.3f", locale: posix, residual)) dB"
        case .alike: "Both play at one level"
        case .measuredAlike: "Measured through both, the two play at one level"
        case .lowered(let arm, let db):
            "\(arm == .a ? "A" : "B") lowered by \(String(format: "%.2f", locale: posix, db)) dB to match the other"
        }
    }

    private static let posix = Locale(identifier: "en_US_POSIX")

    public static func score(_ standing: TestStanding) -> String {
        let chance = String(format: "%.1f", locale: posix, standing.chancePercent)
        return "\(standing.right) of \(standing.answered) right, chance of that by guessing \(chance)%"
    }

    public static func standing(_ standing: TestStanding) -> String {
        if standing.answered == 0 { return "Trial 1 of \(standing.planned)" }
        let trial = standing.finished ? "Finished" : "Trial \(standing.answered + 1) of \(standing.planned)"
        return "\(trial) — \(score(standing))"
    }

    public static func heard(_ standing: TestStanding) -> String {
        let hearing =
            switch standing.heard {
            case .a: "Hearing A"
            case .b: "Hearing B"
            case nil: "Hearing nothing chosen yet"
            }
        return "\(levels(standing.levels)). \(hearing)"
    }

    public static func verdicts(_ standing: TestStanding) -> String {
        standing.verdicts.map { $0 ? "✓" : "✗" }.joined(separator: " ")
    }

    public static func last(_ standing: TestStanding) -> String? {
        guard !standing.finished, let last = standing.verdicts.last else { return nil }
        return last ? "The last one was right" : "The last one was wrong"
    }

    public static func conclusion(_ standing: TestStanding) -> String {
        standing.significant
            ? "That is unlikely to be luck: you can hear this."
            : "That could well be luck: no difference was shown."
    }
}
