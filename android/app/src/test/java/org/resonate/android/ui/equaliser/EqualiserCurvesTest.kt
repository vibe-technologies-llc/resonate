package org.resonate.android.ui.equaliser

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.resonate.android.nativecore.EqualiserBand
import org.resonate.android.nativecore.EqualiserCurve
import org.resonate.android.nativecore.EqualiserFilter
import org.resonate.android.nativecore.EqualiserLimits
import org.resonate.android.nativecore.EqualiserPreset
import org.resonate.android.nativecore.GainMode
import org.resonate.android.nativecore.LossyRestoration
import org.resonate.android.nativecore.PlaybackPreferences
import org.resonate.android.nativecore.ResamplingPhase
import org.resonate.android.nativecore.ResamplingQuality

class EqualiserCurvesTest {
    private fun preset(name: String, detail: String, preamp: Float, gains: List<Float>) =
        EqualiserPreset(
            name,
            detail,
            EqualiserCurve(
                if (gains.isEmpty()) "" else name,
                preamp,
                gains.mapIndexed { index, gain ->
                    EqualiserBand(EqualiserFilter.PEAKING, octaveCentres[index], gain, 1.4142135f)
                },
            ),
        )

    private val presets =
        listOf(
            preset("Flat", "An uncoloured starting point.", 0f, emptyList()),
            preset("Bass boost", "More weight and punch in the low end.", -6f, listOf(4f, 4f, 2f, 0f, -1f, -1f, 0f, 0f, 0f, 0f)),
            preset("Bass only", "More bass.", 0f, listOf(4f, 4f, 2f, 0f, 0f, 0f, 0f, 0f, 0f, 0f)),
            preset("Bass cut", "Less rumble.", 0f, listOf(-5f, -4f, -3f, -1f, 0f, 0f, 0f, 0f, 0f, 0f)),
            preset("Vocals", "Voices forward.", -5f, listOf(-2f, -1f, 0f, 1f, 2f, 3f, 3f, 1f, 0f, -1f)),
            preset("Warm", "Fuller lows.", -5f, listOf(2f, 3f, 2f, 1f, 0f, 0f, -1f, -2f, -2f, -2f)),
            preset("Bright", "More sparkle.", -5f, listOf(-1f, -1f, 0f, 0f, 0f, 1f, 2f, 3f, 3f, 2f)),
            preset("Rock", "Punchy lows.", -5f, listOf(3f, 2f, 0f, -1f, -2f, 0f, 2f, 3f, 3f, 2f)),
            preset("Electronic", "Deep bass.", -6f, listOf(4f, 3f, 1f, 0f, -2f, -1f, 1f, 2f, 3f, 3f)),
        )

    private fun preferences(curve: EqualiserCurve = flatCurve, enabled: Boolean = false) =
        PlaybackPreferences(
            equaliserEnabled = enabled,
            equaliser = curve,
            replayGain = GainMode.OFF,
            quality = ResamplingQuality.HIGH,
            phase = ResamplingPhase.LINEAR,
            truePeak = false,
            restoration = LossyRestoration.OFF,
            bufferMs = 500u,
            dsdLikePcm = false,
        )

    private val imported =
        EqualiserCurve(
            "My headphones",
            -4f,
            listOf(
                EqualiserBand(EqualiserFilter.LOW_SHELF, 100f, 3f, 0.7f),
                EqualiserBand(EqualiserFilter.NOTCH, 2000f, 0f, 4f),
            ),
        )

    private val bounds = EqualiserBounds.unread

    @Test
    fun everyPresetTurnsTheEqualiserOnAndIsShownAgainOnceStored() {
        for (chosen in presets) {
            val stored = preferences().withPreset(chosen)

            assertTrue(stored.equaliserEnabled)
            assertEquals(if (chosen.name == "Flat") 0 else 10, stored.equaliser.bands.size)
            assertTrue(stored.equaliser.preampDb <= 0f)
            assertEquals(chosen, shownPreset(presets, stored.equaliser))
        }
    }

    @Test
    fun bassOnlyRaisesTheLowsWithoutLoweringAnyOtherBandOrThePreamp() {
        val before = preferences(EqualiserCurve("Old profile", -8f, emptyList()))

        val after = before.withPreset(presets.first { it.name == "Bass only" })

        assertEquals(0f, after.equaliser.preampDb)
        assertEquals(10, after.equaliser.bands.size)
        after.equaliser.bands.forEach { band ->
            if (band.frequencyHz <= 125f) assertTrue(band.gainDb > 0f) else assertEquals(0f, band.gainDb)
        }
        assertEquals("Bass only", shownPreset(presets, after.equaliser)?.name)
    }

    @Test
    fun aCurveWithNoGainAnywhereReadsAsFlatAndAnUnnamedEditAsCustom() {
        assertEquals(FLAT_NAME, shownName(flatCurve))
        assertEquals(FLAT_NAME, shownName(octaveCurve(octaveCentres)))
        assertEquals(CUSTOM_NAME, shownName(imported.withPreamp(-3f, bounds)))
        assertEquals("My headphones", shownName(imported))
        assertNull(shownPreset(presets, imported))
    }

    @Test
    fun anEmptyFlatCurveOffersTenOctaveSlidersAndCreatesBandsOnlyWhenOneIsCommitted() {
        val stored = EqualiserCurve("", -2f, emptyList())

        val shown = editable(stored, emptyList())

        assertTrue(stored.bands.isEmpty())
        assertEquals(octaveCentres, shown.bands.map { it.frequencyHz })
        assertTrue(shown.bands.all { it.filter == EqualiserFilter.PEAKING && it.q == OCTAVE_Q && it.gainDb == 0f })
        assertEquals(-2f, shown.preampDb)

        val committed = shown.withGain(2, 3.5f, bounds)

        assertEquals(10, committed.bands.size)
        assertEquals(3.5f, committed.bands[2].gainDb)
        assertEquals(-2f, committed.preampDb)
    }

    @Test
    fun aSliderEditKeepsAnImportedProfilesFiltersFrequenciesQAndPreamp() {
        val shown = editable(imported, octaveCentres)

        val committed = shown.withGain(0, 4.26f, bounds)

        assertEquals(imported, shown)
        assertEquals("", committed.name)
        assertEquals(-4f, committed.preampDb)
        assertEquals(2, committed.bands.size)
        assertEquals(EqualiserFilter.LOW_SHELF, committed.bands[0].filter)
        assertEquals(100f, committed.bands[0].frequencyHz)
        assertEquals(0.7f, committed.bands[0].q)
        assertEquals(4.3f, committed.bands[0].gainDb)
        assertEquals(imported.bands[1], committed.bands[1])
    }

    @Test
    fun sliderNamesSayTheFrequencyAndWhatItSoundsLike() {
        assertEquals("100 Hz Bass", bandName(imported.bands[0]))
        assertEquals("2.00 kHz Vocals", bandName(imported.bands[1]))
        assertEquals("31.3 Hz Sub-bass", bandName(EqualiserBand(EqualiserFilter.PEAKING, 31.25f, 0f, 1f)))
        assertEquals("16.0 kHz Air", bandName(EqualiserBand(EqualiserFilter.PEAKING, 16_000f, 0f, 1f)))
    }

    @Test
    fun fittingThePreampKeepsEveryBandAndClearsTheName() {
        val fitted = imported.copy(preampDb = 2f).withFittedPreamp(-6.25f, bounds)

        assertEquals(-6.25f, fitted.preampDb)
        assertEquals(imported.bands, fitted.bands)
        assertEquals("", fitted.name)
    }

    @Test
    fun theReachOfThePreampIsTheBoundary() {
        assertEquals(-24f, imported.withPreamp(-40f, bounds).preampDb)
        assertEquals(24f, imported.withPreamp(30f, bounds).preampDb)
    }

    @Test
    fun aResetRemovesEveryBandAndLeavesTheSwitchAsItWas() {
        val off = preferences(imported, enabled = false).reset()
        val on = preferences(imported, enabled = true).reset()

        assertFalse(off.equaliserEnabled)
        assertTrue(on.equaliserEnabled)
        assertEquals(EqualiserCurve("", 0f, emptyList()), off.equaliser)
    }

    @Test
    fun anEditTurnsTheEqualiserOnOnlyWhereAsked() {
        val edited = imported.withGain(0, 1f, bounds)

        assertFalse(preferences().withCurve(edited, enable = false).equaliserEnabled)
        assertTrue(preferences().withCurve(edited, enable = true).equaliserEnabled)
        assertTrue(preferences(enabled = true).withCurve(edited, enable = false).equaliserEnabled)
    }

    @Test
    fun aBandIsAddedTidyAndWithinTheBoundsUntilThereIsNoRoom() {
        val added = imported.withAddedBand(23_456f, 40f, bounds)

        assertEquals(3, added?.bands?.size)
        assertEquals(EqualiserBand(EqualiserFilter.PEAKING, 20_000f, 24f, 1f), added?.bands?.last())
        assertEquals("", added?.name)

        val full = EqualiserCurve("", 0f, List(bounds.bandsAtMost) { EqualiserBand(EqualiserFilter.PEAKING, 1000f, 0f, 1f) })
        assertNull(full.withAddedBand(1000f, 0f, bounds))
    }

    @Test
    fun aRemovedBandLeavesTheOthersInOrder() {
        val removed = imported.withoutBand(0)

        assertEquals(listOf(imported.bands[1]), removed.bands)
        assertEquals(imported, imported.withoutBand(5))
    }

    @Test
    fun valuesAreTidiedAsTheyAreShown() {
        assertEquals(31.3f, tidyHertz(31.25f))
        assertEquals(457f, tidyHertz(456.7f))
        assertEquals(1230f, tidyHertz(1234f))
        assertEquals(-1.3f, tidyDb(-1.26f))
        assertEquals(0.71f, tidyQ(0.7071f))
        assertEquals(2.25f, quarterDb(2.3f))
    }

    @Test
    fun hertzAndDecibelsReadAsTheyShould() {
        assertEquals("62.5 Hz", hertzLabel(62.5f))
        assertEquals("250 Hz", hertzLabel(250f))
        assertEquals("2.00 kHz", hertzLabel(2000f))
        assertEquals("16.0 kHz", hertzLabel(16_000f))
        assertEquals("31", shortHertz(31.25f))
        assertEquals("1k", shortHertz(1000f))
        assertEquals("16k", shortHertz(16_000f))
        assertEquals("1.5k", shortHertz(1500f))
        assertEquals("+3.0 dB", signedDb(3f))
        assertEquals("-4.5 dB", signedDb(-4.5f))
        assertEquals("0.0 dB", signedDb(-0.01f))
    }

    @Test
    fun onlyPeakAndShelfFiltersCarryGain() {
        val gained = EqualiserFilter.entries.filter { it.usesGain }

        assertEquals(listOf(EqualiserFilter.PEAKING, EqualiserFilter.LOW_SHELF, EqualiserFilter.HIGH_SHELF), gained)
        assertEquals(-4f, level(imported.bands[1], -4f))
        assertEquals(-1f, level(imported.bands[0], -4f))
    }

    @Test
    fun theSliderReachGrowsOnlyForALouderBand() {
        assertEquals(12f, bankCap(imported, bounds))
        assertEquals(18f, bankCap(imported.withGain(0, -18f, bounds), bounds))
        assertNotEquals(30f, bankCap(imported.withGain(0, 30f, bounds), bounds))
    }

    @Test
    fun boundsFallBackOnlyWhereTheBoundaryGaveNone() {
        val limits =
            EqualiserLimits(
                bandsAtMost = 0u,
                lowestHz = 10f,
                highestHz = 0f,
                gainAtMostDb = 12f,
                preampAtMostDb = 0f,
                qAtLeast = 0.2f,
                qAtMost = 0f,
            )

        val read = EqualiserBounds.of(limits)

        assertEquals(32, read.bandsAtMost)
        assertEquals(10f, read.lowestHz)
        assertEquals(20_000f, read.highestHz)
        assertEquals(12f, read.gainAtMostDb)
        assertEquals(24f, read.preampAtMostDb)
        assertEquals(0.2f, read.qAtLeast)
        assertEquals(40f, read.qAtMost)
    }
}
