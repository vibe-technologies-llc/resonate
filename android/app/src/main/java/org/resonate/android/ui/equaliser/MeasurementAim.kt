package org.resonate.android.ui.equaliser

import java.util.Locale
import org.resonate.android.nativecore.BuiltInTarget
import org.resonate.android.nativecore.MeasurementFit
import org.resonate.android.nativecore.MeasurementOffers
import org.resonate.android.nativecore.MeasurementTarget

sealed interface Aim {
    data class BuiltIn(val name: String) : Aim

    class Document(val name: String, val bytes: ByteArray) : Aim
}

data class MeasurementView(
    val offers: MeasurementOffers?,
    val aim: Aim?,
    val fitting: Boolean,
    val said: String?,
) {
    fun aimed(at: Aim) = copy(aim = at)

    fun begun() = copy(fitting = true, said = null)

    fun finished(said: String?) = copy(fitting = false, said = said ?: this.said)

    companion object {
        val unread = MeasurementView(offers = null, aim = null, fitting = false, said = null)

        fun of(offers: MeasurementOffers) =
            MeasurementView(
                offers = offers,
                aim = (offers.targets.firstOrNull { it.recommended } ?: offers.targets.firstOrNull())?.let { Aim.BuiltIn(it.name) },
                fitting = false,
                said = null,
            )
    }
}

fun Aim.target(): MeasurementTarget =
    when (this) {
        is Aim.BuiltIn -> MeasurementTarget.BuiltIn(name)
        is Aim.Document -> MeasurementTarget.Document(bytes)
    }

fun Aim.label(targets: List<BuiltInTarget>): String =
    when (this) {
        is Aim.BuiltIn -> targets.firstOrNull { it.name == name }?.label ?: name
        is Aim.Document -> name
    }

fun profileNameFor(documentName: String): String =
    documentName.substringBeforeLast('.').trim().ifEmpty { documentName.trim() }

fun fitSentence(fit: MeasurementFit, toward: String): String {
    val count = if (fit.bands == 1u) "1 band" else "${fit.bands} bands"
    val before = String.format(Locale.ROOT, "%.1f", fit.offByDbBefore)
    val after = String.format(Locale.ROOT, "%.1f", fit.offByDbAfter)
    val passed = fit.imported.passedOver
    val skipped =
        when {
            passed == 0u -> ""
            passed == 1u -> " One line of the file was passed over."
            else -> " $passed lines of the file were passed over."
        }
    return "Fitted $count toward $toward: off by $before dB before, $after dB after.$skipped"
}
