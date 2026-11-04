package org.resonate.android

import android.graphics.Bitmap
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.resonate.android.ui.theme.SEED_OUTLASTS_ITS_COVER_MS
import org.resonate.android.ui.theme.rememberSeedOf

@RunWith(AndroidJUnit4::class)
class ArtworkSeedTest {
    @get:Rule val rule = createComposeRule()

    private fun cover(argb: Int): Bitmap =
        Bitmap.createBitmap(96, 96, Bitmap.Config.ARGB_8888).apply { eraseColor(argb) }

    @Test
    fun aColourOutlastsItsCoverThroughASongChangeAndGoesOnceNoCoverComes() {
        var bitmap by mutableStateOf<Bitmap?>(cover(0xFFD02020.toInt()))
        var seed: Color? = null
        rule.setContent { seed = rememberSeedOf(bitmap) }
        rule.waitUntil(WITHIN_MS) { seed != null }
        val held = seed

        rule.mainClock.autoAdvance = false
        bitmap = null
        rule.mainClock.advanceTimeBy(SEED_OUTLASTS_ITS_COVER_MS / 2)
        assertEquals(held, seed)

        rule.mainClock.advanceTimeBy(SEED_OUTLASTS_ITS_COVER_MS)
        assertNull(seed)
    }

    @Test
    fun aCoverWhoseColourIsKnownWearsItOnTheFirstFrame() {
        val first = cover(0xFF2040D0.toInt())
        val second = cover(0xFF20C040.toInt())
        var bitmap by mutableStateOf<Bitmap?>(first)
        var seed: Color? = null
        rule.setContent { seed = rememberSeedOf(bitmap) }
        rule.waitUntil(WITHIN_MS) { seed != null }
        val firstSeed = seed
        assertNotNull(firstSeed)

        bitmap = second
        rule.waitUntil(WITHIN_MS) { seed != firstSeed }
        rule.mainClock.autoAdvance = false

        bitmap = first
        rule.mainClock.advanceTimeByFrame()
        assertEquals(firstSeed, seed)
    }

    private companion object {
        const val WITHIN_MS = 5_000L
    }
}
