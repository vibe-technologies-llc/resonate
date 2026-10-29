import Foundation
import ResonateNative

public enum ListeningTestWords {
    public static let howItWorks =
        "A is the equaliser as set, B is the signal with it bypassed. Each is lowered, never raised, until their loudness across 20 Hz to 16 kHz agrees to 0.1 dB, so the louder one cannot give itself away. X is one of the two, drawn afresh for every trial: hear A, B and X as often as you like on the song playing, then say which X was. A change is heard a few moments after you press, within the audio already buffered. The room correction stays on in both. Editing the curve ends the comparison."

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
        let residual = String(format: "%.3f", locale: posix, standing.residualDb)
        let hearing =
            switch standing.heard {
            case .a: "Hearing A"
            case .b: "Hearing B"
            case nil: "Hearing nothing chosen yet"
            }
        return "Levels matched to \(residual) dB. \(hearing)"
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
