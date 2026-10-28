package org.resonate.android.ui.equaliser

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.resonate.android.nativecore.BuiltInTarget
import org.resonate.android.nativecore.EqualiserCurve
import org.resonate.android.nativecore.EqualiserImport
import org.resonate.android.nativecore.MeasurementFit
import org.resonate.android.nativecore.MeasurementOffers
import org.resonate.android.nativecore.MeasurementTarget

class MeasurementAimTest {
    private val flat = BuiltInTarget("flat", "Flat", false)
    private val overEar = BuiltInTarget("harman-over-ear-2018", "Harman over-ear 2018", true)
    private val inEar = BuiltInTarget("harman-in-ear-2019", "Harman in-ear 2019", false)
    private val offers = MeasurementOffers(listOf(flat, overEar, inEar), 4_194_304uL)

    private fun fit(bands: UInt, before: Float, after: Float, passedOver: UInt = 0u) =
        MeasurementFit(
            EqualiserImport(EqualiserCurve("Mine", -3f, emptyList()), passedOver, 0u, 0u, false),
            bands,
            before,
            after,
        )

    @Test
    fun theRecommendedBuiltInIsAimedAtFirstAndNothingBeforeTheOffersAreRead() {
        assertEquals(Aim.BuiltIn("harman-over-ear-2018"), MeasurementView.of(offers).aim)
        assertNull(MeasurementView.unread.aim)
        assertNull(MeasurementView.unread.offers)
        assertEquals(
            Aim.BuiltIn("flat"),
            MeasurementView.of(MeasurementOffers(listOf(flat, inEar), 1uL)).aim,
        )
        assertNull(MeasurementView.of(MeasurementOffers(emptyList(), 1uL)).aim)
    }

    @Test
    fun aBuiltInIsAskedForByNameAndADocumentByItsBytes() {
        val bytes = byteArrayOf(49, 48, 48)

        assertEquals(MeasurementTarget.BuiltIn("flat"), Aim.BuiltIn("flat").target())
        assertEquals(
            MeasurementTarget.Document(bytes).bytes.toList(),
            (Aim.Document("mine.csv", bytes).target() as MeasurementTarget.Document).bytes.toList(),
        )
    }

    @Test
    fun anAimIsLabelledByTheBuiltInsLabelOrTheFilesName() {
        val targets = offers.targets

        assertEquals("Harman in-ear 2019", Aim.BuiltIn("harman-in-ear-2019").label(targets))
        assertEquals("unknown", Aim.BuiltIn("unknown").label(targets))
        assertEquals("target.csv", Aim.Document("target.csv", ByteArray(0)).label(targets))
    }

    @Test
    fun aProfileIsNamedAfterTheMeasurementFileWithoutItsExtension() {
        assertEquals("HD 650", profileNameFor("HD 650.csv"))
        assertEquals("left.ear", profileNameFor("left.ear.txt"))
        assertEquals("sweep", profileNameFor("sweep"))
        assertEquals(".txt", profileNameFor(".txt"))
        assertEquals("", profileNameFor("  "))
    }

    @Test
    fun theFitIsToldWithItsBandsItsTargetAndTheErrorBeforeAndAfter() {
        assertEquals(
            "Fitted 7 bands toward Harman over-ear 2018: off by 5.1 dB before, 1.2 dB after.",
            fitSentence(fit(7u, 5.14f, 1.2f), "Harman over-ear 2018"),
        )
        assertEquals(
            "Fitted 1 band toward Flat: off by 2.0 dB before, 0.5 dB after.",
            fitSentence(fit(1u, 2f, 0.5f), "Flat"),
        )
    }

    @Test
    fun linesThatWereNoPointAreMentionedOnlyWhenThereWereSome() {
        assertTrue(fitSentence(fit(3u, 2f, 1f, 1u), "Flat").endsWith(" One line of the file was passed over."))
        assertTrue(fitSentence(fit(3u, 2f, 1f, 12u), "Flat").endsWith(" 12 lines of the file were passed over."))
        assertFalse(fitSentence(fit(3u, 2f, 1f), "Flat").contains("passed over"))
    }

    @Test
    fun aFitInProgressClearsTheLastSentenceAndKeepsTheNewOneWhenItEnds() {
        val said = MeasurementView.of(offers).copy(said = "Fitted earlier")

        val begun = said.begun()
        val done = begun.finished("Fitted now")
        val failed = begun.finished(null)

        assertTrue(begun.fitting)
        assertNull(begun.said)
        assertFalse(done.fitting)
        assertEquals("Fitted now", done.said)
        assertFalse(failed.fitting)
        assertNull(failed.said)
        assertEquals(done.copy(fitting = false), done.finished(null))
    }
}
