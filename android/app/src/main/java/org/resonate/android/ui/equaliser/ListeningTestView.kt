package org.resonate.android.ui.equaliser

import androidx.annotation.StringRes
import java.util.Locale
import org.resonate.android.R
import org.resonate.android.Words
import org.resonate.android.nativecore.ListeningTest
import org.resonate.android.nativecore.SettingKind
import org.resonate.android.nativecore.TestArm
import org.resonate.android.nativecore.TestLevels
import org.resonate.android.nativecore.TestStanding
import org.resonate.android.nativecore.TestedDither
import org.resonate.android.nativecore.TestedSetting
import org.resonate.android.ui.settings.Choice
import org.resonate.android.ui.settings.labelOf
import org.resonate.android.ui.settings.phaseChoices
import org.resonate.android.ui.settings.qualityChoices
import org.resonate.android.ui.settings.restorationChoices

enum class Testing(@StringRes val label: Int, val kind: SettingKind?, @StringRes val how: Int) {
    Equaliser(R.string.equaliser_test_equaliser, null, R.string.equaliser_test_equaliser_how),
    Restoration(
        R.string.equaliser_test_restoration,
        SettingKind.RESTORATION,
        R.string.equaliser_test_restoration_how,
    ),
    Resampling(
        R.string.equaliser_test_resampler,
        SettingKind.RESAMPLING,
        R.string.equaliser_test_resampler_how,
    ),
    Dither(R.string.equaliser_test_dither, SettingKind.DITHER, R.string.equaliser_test_dither_how),
}

fun howWords(words: Words, testing: Testing): String =
    words.string(testing.how, words.string(R.string.equaliser_test_how_x))

class ListeningRun(val test: ListeningTest, val standing: TestStanding, val testing: Testing)

data class ListeningTestView(
    val lengths: List<UInt>,
    val run: ListeningRun?,
    val beginning: Boolean,
    val testing: Testing = Testing.Equaliser,
    val against: TestedSetting? = null,
) {
    fun beginning() = copy(beginning = true)

    fun begun(test: ListeningTest, testing: Testing) =
        copy(run = ListeningRun(test, test.standing(), testing), beginning = false)

    fun failedToBegin() = copy(beginning = false)

    fun stood(standing: TestStanding) =
        run?.let { copy(run = ListeningRun(it.test, standing, it.testing)) } ?: this

    fun ended() = copy(run = null, beginning = false)

    fun testing(testing: Testing, against: TestedSetting?) =
        copy(testing = testing, against = against)

    fun against(against: TestedSetting) = copy(against = against)

    companion object {
        val unread = ListeningTestView(lengths = emptyList(), run = null, beginning = false)
    }
}

fun ditherChoices(words: Words) =
    listOf(
        Choice(TestedDither.OFF, words.string(R.string.equaliser_test_dither_none)),
        Choice(TestedDither.FLAT, words.string(R.string.equaliser_test_dither_triangular)),
        Choice(TestedDither.SHAPED, words.string(R.string.equaliser_test_dither_shaped)),
    )

fun beginningWords(words: Words, testing: Testing): String =
    words.string(
        if (testing == Testing.Restoration) R.string.equaliser_test_measuring
        else R.string.equaliser_test_starting
    )

fun scoreWords(words: Words, standing: TestStanding): String {
    val chance = String.format(Locale.ROOT, "%.1f", standing.chancePercent)
    return words.string(
        R.string.equaliser_test_score,
        standing.right.toInt(),
        standing.answered.toInt(),
        chance,
    )
}

fun standingWords(words: Words, standing: TestStanding): String {
    val planned = standing.planned.toInt()
    if (standing.answered == 0u) return words.string(R.string.equaliser_test_trial, 1, planned)
    val trial =
        if (standing.finished) words.string(R.string.equaliser_test_finished)
        else words.string(R.string.equaliser_test_trial, standing.answered.toInt() + 1, planned)
    return "$trial — ${scoreWords(words, standing)}"
}

fun levelsWords(words: Words, levels: TestLevels): String =
    when (levels) {
        is TestLevels.Matched ->
            words.string(
                R.string.equaliser_test_levels_matched,
                String.format(Locale.ROOT, "%.3f", levels.residualDb),
            )
        TestLevels.Alike -> words.string(R.string.equaliser_test_levels_alike)
        TestLevels.MeasuredAlike -> words.string(R.string.equaliser_test_levels_measured_alike)
        is TestLevels.Lowered ->
            words.string(
                R.string.equaliser_test_levels_lowered,
                levels.arm.name,
                String.format(Locale.ROOT, "%.2f", levels.db),
            )
    }

fun heardWords(words: Words, standing: TestStanding): String {
    val hearing =
        when (standing.heard) {
            TestArm.A -> words.string(R.string.equaliser_test_hearing_a)
            TestArm.B -> words.string(R.string.equaliser_test_hearing_b)
            null -> words.string(R.string.equaliser_test_hearing_nothing)
        }
    return "${levelsWords(words, standing.levels)}. $hearing"
}

fun playedAsWords(words: Words, setting: TestedSetting): String =
    when (setting) {
        is TestedSetting.Restoration ->
            words.string(
                R.string.equaliser_test_a_is,
                labelOf(restorationChoices(words), setting.a),
            )
        is TestedSetting.Resampling ->
            words.string(
                R.string.equaliser_test_a_is_both,
                labelOf(qualityChoices(words), setting.aQuality),
                labelOf(phaseChoices(words), setting.aPhase),
            )
        is TestedSetting.Dither ->
            words.string(R.string.equaliser_test_a_is, labelOf(ditherChoices(words), setting.a))
    }

fun verdictsInALine(standing: TestStanding): String =
    standing.verdicts.joinToString(" ") { if (it) "✓" else "✗" }

fun lastWords(words: Words, standing: TestStanding): String? =
    standing.verdicts.lastOrNull()?.takeIf { !standing.finished }?.let {
        words.string(
            if (it) R.string.equaliser_test_last_right else R.string.equaliser_test_last_wrong
        )
    }

fun conclusionWords(words: Words, standing: TestStanding): String =
    words.string(
        if (standing.significant) R.string.equaliser_test_heard
        else R.string.equaliser_test_luck
    )
