package org.resonate.android.ui.equaliser

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import org.resonate.android.nativecore.EqualiserBand
import org.resonate.android.nativecore.EqualiserCurve
import org.resonate.android.nativecore.EqualiserFilter
import org.resonate.android.nativecore.EqualiserImport

class EntryTest {
    private val bounds = EqualiserBounds.unread

    @Test
    fun aTypedLevelReadsWithItsSignUnitAndEitherDecimalMark() {
        assertEquals(3f, typedNumber("3"))
        assertEquals(3f, typedNumber("+3 dB"))
        assertEquals(-4.5f, typedNumber("-4,5"))
        assertEquals(-4.5f, typedNumber("−4.5dB"))
        assertEquals(1.41f, typedNumber("Q 1.41"))
        assertNull(typedNumber(""))
        assertNull(typedNumber("loud"))
        assertNull(typedNumber("12k"))
        assertNull(typedNumber("NaN"))
    }

    @Test
    fun aTypedFrequencyReadsInHertzOrKilohertz() {
        assertEquals(250f, typedHertz("250"))
        assertEquals(250f, typedHertz("250 Hz"))
        assertEquals(1500f, typedHertz("1.5 kHz"))
        assertEquals(1500f, typedHertz("1,5k"))
        assertEquals(2000f, typedHertz("2KHZ"))
        assertNull(typedHertz("kHz"))
    }

    @Test
    fun aValueIsAcceptedOnlyWithinItsMeasure() {
        assertEquals(-24f, gainMeasure(bounds).accepted("-24"))
        assertNull(gainMeasure(bounds).accepted("24.5"))
        assertEquals(20_000f, hertzMeasure(bounds).accepted("20k"))
        assertNull(hertzMeasure(bounds).accepted("19"))
        assertEquals(0.1f, widthMeasure(bounds).accepted("0.1"))
        assertNull(widthMeasure(bounds).accepted("41"))
        assertEquals("From -24.0 dB to +24.0 dB", gainMeasure(bounds).reach)
        assertEquals("From 20.0 Hz to 20.0 kHz", hertzMeasure(bounds).reach)
    }

    @Test
    fun theFieldOpensOnTheValueAsShortAsItReads() {
        assertEquals("3", gainMeasure(bounds).written(3f))
        assertEquals("-4.5", gainMeasure(bounds).written(-4.5f))
        assertEquals("0", gainMeasure(bounds).written(-0.01f))
        assertEquals("1230", hertzMeasure(bounds).written(1234f))
        assertEquals("31.3", hertzMeasure(bounds).written(31.25f))
        assertEquals("1.41", widthMeasure(bounds).written(OCTAVE_Q))
    }

    @Test
    fun theSignIsFlippedWhateverWasTyped() {
        assertEquals("-3", flippedSign("3"))
        assertEquals("3", flippedSign("-3"))
        assertEquals("-3", flippedSign("+3"))
        assertEquals("-", flippedSign(""))
    }

    @Test
    fun anImportSaysWhatItLoadedAndWhatItGaveUp() {
        val curve =
            EqualiserCurve(
                "HD 650",
                -6f,
                listOf(
                    EqualiserBand(EqualiserFilter.PEAKING, 100f, 3f, 0.7f),
                    EqualiserBand(EqualiserFilter.HIGH_SHELF, 8000f, -2f, 0.7f),
                ),
            )

        assertEquals(
            "Loaded 2 bands from HD 650.",
            importSentence(EqualiserImport(curve, 0u, 0u, 0u, false)),
        )
        assertEquals(
            "Loaded 2 bands from HD 650, fitted from its GraphicEQ line. 1 value was held to the equaliser's reach; 3 lines were passed over.",
            importSentence(EqualiserImport(curve, 3u, 0u, 1u, true)),
        )
        assertEquals(
            "Loaded 1 band from the profile. 2 filters were approximated.",
            importSentence(
                EqualiserImport(EqualiserCurve("", 0f, curve.bands.take(1)), 0u, 2u, 0u, false)
            ),
        )
    }
}
