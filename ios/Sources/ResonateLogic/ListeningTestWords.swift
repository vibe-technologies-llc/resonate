import Foundation
import ResonateNative

public enum ListeningTesting: CaseIterable, Sendable {
    case equaliser
    case restoration
    case resampling
    case dither

    public var label: String {
        switch self {
        case .equaliser: said("listening-test.testing.equaliser")
        case .restoration: said("listening-test.testing.restoration")
        case .resampling: said("listening-test.testing.resampler")
        case .dither: said("listening-test.testing.dither")
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
            said("listening-test.how.equaliser", ListeningTestWords.howXIsDrawn)
        case .restoration:
            said("listening-test.how.restoration", ListeningTestWords.howXIsDrawn)
        case .resampling:
            said("listening-test.how.resampling", ListeningTestWords.howXIsDrawn)
        case .dither:
            said("listening-test.how.dither", ListeningTestWords.howXIsDrawn)
        }
    }
}

public enum ListeningTestWords {
    static var howXIsDrawn: String { said("listening-test.how.x") }

    public static let ditherChoices: [TestedDither] = [.off, .flat, .shaped]

    public static func restorationLabel(_ value: LossyRestoration) -> String {
        switch value {
        case .off: said("listening-test.restoration.off")
        case .repair: said("listening-test.restoration.repair")
        case .extend: said("listening-test.restoration.extend")
        }
    }

    public static func qualityLabel(_ value: ResamplingQuality) -> String {
        switch value {
        case .lowLatency: said("listening-test.quality.low-latency")
        case .balanced: said("listening-test.quality.balanced")
        case .high: said("listening-test.quality.high")
        case .veryHigh: said("listening-test.quality.very-high")
        }
    }

    public static func phaseLabel(_ value: ResamplingPhase) -> String {
        switch value {
        case .linear: said("listening-test.phase.linear")
        case .intermediate: said("listening-test.phase.intermediate")
        case .minimum: said("listening-test.phase.minimum")
        }
    }

    public static func ditherLabel(_ value: TestedDither) -> String {
        switch value {
        case .off: said("listening-test.dither.none")
        case .flat: said("listening-test.dither.triangular")
        case .shaped: said("listening-test.dither.shaped")
        }
    }

    public static func playedAs(_ setting: TestedSetting) -> String {
        switch setting {
        case .restoration(let a, _): said("listening-test.played-as", restorationLabel(a))
        case .resampling(let quality, let phase, _, _):
            said("listening-test.played-as.resampling", qualityLabel(quality), phaseLabel(phase))
        case .dither(let a, _): said("listening-test.played-as", ditherLabel(a))
        }
    }

    public static func beginning(_ testing: ListeningTesting) -> String {
        testing == .restoration ? said("listening-test.measuring") : said("listening-test.starting")
    }

    public static func levels(_ levels: TestLevels) -> String {
        switch levels {
        case .matched(let residual): said("listening-test.levels.matched", String(format: "%.3f", locale: posix, residual))
        case .alike: said("listening-test.levels.alike")
        case .measuredAlike: said("listening-test.levels.measured-alike")
        case .lowered(let arm, let db):
            said(
                arm == .a ? "listening-test.levels.lowered-a" : "listening-test.levels.lowered-b",
                String(format: "%.2f", locale: posix, db)
            )
        }
    }

    private static let posix = Locale(identifier: "en_US_POSIX")

    public static func score(_ standing: TestStanding) -> String {
        let chance = String(format: "%.1f", locale: posix, standing.chancePercent)
        return said("listening-test.score", Int(standing.right), Int(standing.answered), chance)
    }

    public static func standing(_ standing: TestStanding) -> String {
        if standing.answered == 0 { return said("listening-test.trial", 1, Int(standing.planned)) }
        let trial = standing.finished
            ? said("listening-test.finished")
            : said("listening-test.trial", Int(standing.answered) + 1, Int(standing.planned))
        return "\(trial) — \(score(standing))"
    }

    public static func heard(_ standing: TestStanding) -> String {
        let hearing =
            switch standing.heard {
            case .a: said("listening-test.hearing.a")
            case .b: said("listening-test.hearing.b")
            case nil: said("listening-test.hearing.nothing")
            }
        return "\(levels(standing.levels)). \(hearing)"
    }

    public static func verdicts(_ standing: TestStanding) -> String {
        standing.verdicts.map { $0 ? "✓" : "✗" }.joined(separator: " ")
    }

    public static func last(_ standing: TestStanding) -> String? {
        guard !standing.finished, let last = standing.verdicts.last else { return nil }
        return last ? said("listening-test.last.right") : said("listening-test.last.wrong")
    }

    public static func conclusion(_ standing: TestStanding) -> String {
        standing.significant
            ? said("listening-test.conclusion.heard")
            : said("listening-test.conclusion.luck")
    }
}
