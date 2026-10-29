import Foundation
import ResonateNative
import Testing

@testable import ResonateLogic

@Suite struct ListeningTestWordsTests {
    func standing(
        _ verdicts: [Bool],
        planned: UInt32 = 16,
        heard: TestArm? = nil,
        significant: Bool = false,
        chance: Double = 100,
        levels: TestLevels = .matched(residualDb: 0.0123)
    ) -> TestStanding {
        TestStanding(
            planned: planned,
            answered: UInt32(verdicts.count),
            right: UInt32(verdicts.filter { $0 }.count),
            finished: UInt32(verdicts.count) == planned,
            chancePercent: chance,
            significant: significant,
            verdicts: verdicts,
            levels: levels,
            heard: heard
        )
    }

    @Test func aTestNothingWasAnsweredInAsksTheFirstTrial() {
        #expect(ListeningTestWords.standing(standing([])) == "Trial 1 of 16")
    }

    @Test func aTestInProgressNamesTheNextTrialAndTheScoreWithTheChanceOfItByGuessing() {
        let words = ListeningTestWords.standing(standing([true, false, true], chance: 50))

        #expect(words == "Trial 4 of 16 — 2 of 3 right, chance of that by guessing 50.0%")
    }

    @Test func aFinishedTestSaysSoAndItsConclusionFollowsTheSignificance() {
        let luck = standing([true, false, true, false], planned: 4)
        let heard = standing(Array(repeating: true, count: 8), planned: 8, significant: true, chance: 0.4)

        #expect(ListeningTestWords.standing(luck) == "Finished — 2 of 4 right, chance of that by guessing 100.0%")
        #expect(ListeningTestWords.conclusion(luck) == "That could well be luck: no difference was shown.")
        #expect(ListeningTestWords.conclusion(heard) == "That is unlikely to be luck: you can hear this.")
    }

    @Test func theArmHeardIsNamedAndAnXIsNeverNamed() {
        #expect(ListeningTestWords.heard(standing([], heard: .a)) == "Levels matched to 0.012 dB. Hearing A")
        #expect(ListeningTestWords.heard(standing([], heard: .b)) == "Levels matched to 0.012 dB. Hearing B")
        #expect(
            ListeningTestWords.heard(standing([])) == "Levels matched to 0.012 dB. Hearing nothing chosen yet"
        )
    }

    @Test func theVerdictsAreALineAndTheLastIsToldUntilTheTestIsFinished() {
        let running = standing([true, false])

        #expect(ListeningTestWords.verdicts(running) == "✓ ✗")
        #expect(ListeningTestWords.last(running) == "The last one was wrong")
        #expect(ListeningTestWords.last(standing([false, true])) == "The last one was right")
        #expect(ListeningTestWords.last(standing([])) == nil)
        #expect(ListeningTestWords.last(standing([true, true], planned: 2)) == nil)
    }

    @Test func aSettingTestSaysHowItsLevelsWereMatched() {
        #expect(ListeningTestWords.heard(standing([], heard: .a, levels: .alike)) == "Both play at one level. Hearing A")
        #expect(
            ListeningTestWords.heard(standing([], levels: .measuredAlike))
                == "Measured through both, the two play at one level. Hearing nothing chosen yet"
        )
        #expect(
            ListeningTestWords.heard(standing([], heard: .b, levels: .lowered(arm: .b, db: 0.12)))
                == "B lowered by 0.12 dB to match the other. Hearing B"
        )
    }

    @Test func aSettingIsNamedAsPlayedAndEachTestSaysWhatIsCompared() {
        #expect(ListeningTestWords.playedAs(.restoration(a: .off, b: .extend)) == "A is Off")
        #expect(
            ListeningTestWords.playedAs(.resampling(aQuality: .veryHigh, aPhase: .minimum, bQuality: .fast, bPhase: .minimum))
                == "A is Very high · Minimum"
        )
        #expect(ListeningTestWords.playedAs(.dither(a: .shaped, b: .off)) == "A is Triangular, shaped")
        #expect(Set(ListeningTesting.allCases.map(\.howItWorks)).count == ListeningTesting.allCases.count)
        #expect(ListeningTestWords.beginning(.restoration) == "Measuring…")
        #expect(ListeningTestWords.beginning(.dither) == "Starting…")
        #expect(ListeningTesting.equaliser.kind == nil)
        #expect(ListeningTesting.resampling.kind == .resampling)
    }
}
