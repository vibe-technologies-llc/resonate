package org.resonate.android.ui.equaliser

import java.util.Locale
import org.resonate.android.nativecore.ListeningTest
import org.resonate.android.nativecore.TestArm
import org.resonate.android.nativecore.TestStanding

class ListeningRun(val test: ListeningTest, val standing: TestStanding)

data class ListeningTestView(
    val lengths: List<UInt>,
    val run: ListeningRun?,
    val beginning: Boolean,
) {
    fun beginning() = copy(beginning = true)

    fun begun(test: ListeningTest) =
        copy(run = ListeningRun(test, test.standing()), beginning = false)

    fun failedToBegin() = copy(beginning = false)

    fun stood(standing: TestStanding) = run?.let { copy(run = ListeningRun(it.test, standing)) } ?: this

    fun ended() = copy(run = null, beginning = false)

    companion object {
        val unread = ListeningTestView(lengths = emptyList(), run = null, beginning = false)
    }
}

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

fun heardWords(standing: TestStanding): String {
    val residual = String.format(Locale.ROOT, "%.3f", standing.residualDb)
    val hearing =
        when (standing.heard) {
            TestArm.A -> "Hearing A"
            TestArm.B -> "Hearing B"
            null -> "Hearing nothing chosen yet"
        }
    return "Levels matched to $residual dB. $hearing"
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

const val LISTENING_TEST_HOW_IT_WORKS =
    "A is the equaliser as set, B is the signal with it bypassed. Each is lowered, never raised, until their loudness across 20 Hz to 16 kHz agrees to 0.1 dB, so the louder one cannot give itself away. X is one of the two, drawn afresh for every trial: hear A, B and X as often as you like on the song playing, then say which X was. A change is heard a few moments after you press, within the audio already buffered. The room correction stays on in both. Editing the curve ends the comparison."
