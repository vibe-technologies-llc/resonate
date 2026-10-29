package org.resonate.android.ui.equaliser

import java.util.Locale
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

private const val HOW_X_IS_DRAWN =
    " X is one of the two, drawn afresh for every trial: hear A, B and X as often as you like on the song playing, then say which X was. A change is heard a few moments after you press, within the audio already buffered."

enum class Testing(val label: String, val kind: SettingKind?, val how: String) {
    Equaliser(
        "Equaliser",
        null,
        "A is the equaliser as set, B is the signal with it bypassed. Each is lowered, never raised, until their loudness across 20 Hz to 16 kHz agrees to 0.1 dB, so the louder one cannot give itself away.$HOW_X_IS_DRAWN The room correction stays on in both. Editing the curve ends the comparison.",
    ),
    Restoration(
        "Restoration",
        SettingKind.RESTORATION,
        "A is lossy restoration as set, B the setting you choose. Restoration changes how loud a song plays, so the song is first measured through both, which takes a moment, and the louder is lowered, never raised, until the two agree. A lossless file is touched by neither.$HOW_X_IS_DRAWN",
    ),
    Resampling(
        "Resampler",
        SettingKind.RESAMPLING,
        "A is the resampler as set, B the quality and phase you choose. Both play at one level. The resampler works only where the song's rate has to be changed to the device's; where it does not, the two sound alike and the test is refused.$HOW_X_IS_DRAWN",
    ),
    Dither(
        "Dither",
        SettingKind.DITHER,
        "A is the dither as the phone plays it, B the kind you choose. Both play at one level. Dither works only where the bits are cut down to the device's depth; where they are not, the two sound alike and the test is refused.$HOW_X_IS_DRAWN",
    ),
}

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

val ditherChoices =
    listOf(
        Choice(TestedDither.OFF, "None"),
        Choice(TestedDither.FLAT, "Triangular"),
        Choice(TestedDither.SHAPED, "Triangular, shaped"),
    )

fun beginningWords(testing: Testing): String =
    if (testing == Testing.Restoration) "Measuring…" else "Starting…"

fun scoreWords(standing: TestStanding): String {
    val chance = String.format(Locale.ROOT, "%.1f", standing.chancePercent)
    return "${standing.right} of ${standing.answered} right, chance of that by guessing $chance%"
}

fun standingWords(standing: TestStanding): String {
    if (standing.answered == 0u) return "Trial 1 of ${standing.planned}"
    val trial =
        if (standing.finished) "Finished" else "Trial ${standing.answered + 1u} of ${standing.planned}"
    return "$trial — ${scoreWords(standing)}"
}

fun levelsWords(levels: TestLevels): String =
    when (levels) {
        is TestLevels.Matched ->
            "Levels matched to ${String.format(Locale.ROOT, "%.3f", levels.residualDb)} dB"
        TestLevels.Alike -> "Both play at one level"
        TestLevels.MeasuredAlike -> "Measured through both, the two play at one level"
        is TestLevels.Lowered ->
            "${levels.arm.name} lowered by ${String.format(Locale.ROOT, "%.2f", levels.db)} dB to match the other"
    }

fun heardWords(standing: TestStanding): String {
    val hearing =
        when (standing.heard) {
            TestArm.A -> "Hearing A"
            TestArm.B -> "Hearing B"
            null -> "Hearing nothing chosen yet"
        }
    return "${levelsWords(standing.levels)}. $hearing"
}

fun playedAsWords(setting: TestedSetting): String =
    when (setting) {
        is TestedSetting.Restoration ->
            "A is ${labelOf(restorationChoices, setting.a)}"
        is TestedSetting.Resampling ->
            "A is ${labelOf(qualityChoices, setting.aQuality)} · ${labelOf(phaseChoices, setting.aPhase)}"
        is TestedSetting.Dither -> "A is ${labelOf(ditherChoices, setting.a)}"
    }

fun verdictsInALine(standing: TestStanding): String =
    standing.verdicts.joinToString(" ") { if (it) "✓" else "✗" }

fun lastWords(standing: TestStanding): String? =
    standing.verdicts.lastOrNull()?.takeIf { !standing.finished }?.let {
        if (it) "The last one was right" else "The last one was wrong"
    }

fun conclusionWords(standing: TestStanding): String =
    if (standing.significant) "That is unlikely to be luck: you can hear this."
    else "That could well be luck: no difference was shown."
