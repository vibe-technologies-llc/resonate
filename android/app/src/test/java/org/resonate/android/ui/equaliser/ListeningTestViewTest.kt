package org.resonate.android.ui.equaliser

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import org.resonate.android.nativecore.LossyRestoration
import org.resonate.android.nativecore.TestArm
import org.resonate.android.nativecore.TestLevels
import org.resonate.android.nativecore.TestStanding
import org.resonate.android.nativecore.TestedDither
import org.resonate.android.nativecore.TestedSetting

class ListeningTestViewTest {
    private fun standing(
        verdicts: List<Boolean>,
        planned: UInt = 16u,
        heard: TestArm? = null,
        significant: Boolean = false,
        chancePercent: Double = 100.0,
        levels: TestLevels = TestLevels.Matched(0.0123),
    ) =
        TestStanding(
            planned,
            verdicts.size.toUInt(),
            verdicts.count { it }.toUInt(),
            verdicts.size.toUInt() == planned,
            chancePercent,
            significant,
            verdicts,
            levels,
            heard,
        )

    @Test
    fun aTestNothingWasAnsweredInAsksTheFirstTrial() {
        assertEquals("Trial 1 of 16", standingWords(standing(emptyList())))
    }

    @Test
    fun aTestInProgressNamesTheNextTrialAndTheScoreWithTheChanceOfItByGuessing() {
        val words = standingWords(standing(listOf(true, false, true), chancePercent = 50.0))

        assertEquals("Trial 4 of 16 — 2 of 3 right, chance of that by guessing 50.0%", words)
    }

    @Test
    fun aFinishedTestSaysSoAndItsConclusionFollowsTheSignificance() {
        val luck = standing(List(4) { it % 2 == 0 }, planned = 4u)
        val heard = standing(List(8) { true }, planned = 8u, significant = true, chancePercent = 0.4)

        assertEquals("Finished — 2 of 4 right, chance of that by guessing 100.0%", standingWords(luck))
        assertEquals("That could well be luck: no difference was shown.", conclusionWords(luck))
        assertEquals("That is unlikely to be luck: you can hear this.", conclusionWords(heard))
    }

    @Test
    fun theArmHeardIsNamedAndAnXIsNeverNamed() {
        assertEquals(
            "Levels matched to 0.012 dB. Hearing A",
            heardWords(standing(emptyList(), heard = TestArm.A)),
        )
        assertEquals(
            "Levels matched to 0.012 dB. Hearing B",
            heardWords(standing(emptyList(), heard = TestArm.B)),
        )
        assertEquals(
            "Levels matched to 0.012 dB. Hearing nothing chosen yet",
            heardWords(standing(emptyList())),
        )
    }

    @Test
    fun theVerdictsAreALineAndTheLastIsToldUntilTheTestIsFinished() {
        val running = standing(listOf(true, false))

        assertEquals("✓ ✗", verdictsInALine(running))
        assertEquals("The last one was wrong", lastWords(running))
        assertEquals("The last one was right", lastWords(standing(listOf(false, true))))
        assertNull(lastWords(standing(emptyList())))
        assertNull(lastWords(standing(listOf(true, true), planned = 2u)))
    }

    @Test
    fun aViewHoldsNoRunUntilOneBeginsAndAnEndedOneHoldsNone() {
        val view = ListeningTestView.unread.copy(lengths = listOf(8u, 16u, 32u))

        assertNull(view.run)
        assertEquals(false, view.beginning().failedToBegin().beginning)
        assertEquals(view, view.ended())
        assertEquals(view, view.stood(standing(emptyList())))
    }

    @Test
    fun aSettingTestSaysHowItsLevelsWereMatched() {
        assertEquals(
            "Both play at one level. Hearing A",
            heardWords(standing(emptyList(), levels = TestLevels.Alike, heard = TestArm.A)),
        )
        assertEquals(
            "Measured through both, the two play at one level. Hearing nothing chosen yet",
            heardWords(standing(emptyList(), levels = TestLevels.MeasuredAlike)),
        )
        assertEquals(
            "B lowered by 0.12 dB to match the other. Hearing B",
            heardWords(
                standing(emptyList(), levels = TestLevels.Lowered(TestArm.B, 0.12), heard = TestArm.B)
            ),
        )
    }

    @Test
    fun aSettingIsNamedAsPlayedAndEachTestSaysWhatIsCompared() {
        assertEquals(
            "A is Off",
            playedAsWords(TestedSetting.Restoration(LossyRestoration.OFF, LossyRestoration.EXTEND)),
        )
        assertEquals(
            "A is Triangular, shaped",
            playedAsWords(TestedSetting.Dither(TestedDither.SHAPED, TestedDither.OFF)),
        )
        assertEquals(Testing.entries.size, Testing.entries.map { it.how }.toSet().size)
        assertEquals("Measuring…", beginningWords(Testing.Restoration))
        assertEquals("Starting…", beginningWords(Testing.Dither))
    }

    @Test
    fun theChoiceOfWhatIsTestedIsKeptWithTheSettingItIsTestedAgainst() {
        val view = ListeningTestView.unread.copy(lengths = listOf(8u))
        val against = TestedSetting.Dither(TestedDither.SHAPED, TestedDither.OFF)

        val chosen = view.testing(Testing.Dither, against)

        assertEquals(Testing.Dither, chosen.testing)
        assertEquals(against, chosen.against)
        assertEquals(
            TestedSetting.Dither(TestedDither.SHAPED, TestedDither.FLAT),
            chosen.against(TestedSetting.Dither(TestedDither.SHAPED, TestedDither.FLAT)).against,
        )
    }
}
