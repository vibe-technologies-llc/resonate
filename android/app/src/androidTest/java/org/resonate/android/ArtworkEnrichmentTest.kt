package org.resonate.android

import android.content.Context
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class ArtworkEnrichmentTest {
    @Test
    fun artworkPreferencesPersistAndOfflineRequestsStayStopped() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val library = (context.applicationContext as ResonateApplication).library
        val features = library.features
        val enrichment = features.enrichment
        val settings = context.getSharedPreferences("library-features", Context.MODE_PRIVATE)
        val online = features.online.value
        val automatic = enrichment.automatic.value
        val unmetered = enrichment.unmeteredOnly.value
        ActivityScenario.launch(MainActivity::class.java).use { scenario ->
            try {
                eventually { library.snapshot.value != null }
                features.setOnline(false)
                eventually { !features.online.value }
                runBlocking {
                    enrichment.stop().join()
                    enrichment.setAutomatic(false).join()
                    enrichment.setUnmeteredOnly(true).join()
                }
                assertFalse(settings.getBoolean("enrich-automatically", true))
                assertTrue(settings.getBoolean("enrich-unmetered", false))
                runBlocking {
                    enrichment.setAutomatic(true).join()
                    enrichment.begin().join()
                    enrichment.refresh().join()
                }
                val asked = enrichment.coversAskedAgain.value
                runBlocking { enrichment.retryMissingCovers().join() }
                assertEquals(asked, enrichment.coversAskedAgain.value)
                assertTrue(settings.getBoolean("enrich-automatically", false))
                assertFalse(requireNotNull(enrichment.status.value).running)
                scenario.recreate()
                runBlocking { enrichment.read().join() }
                assertTrue(enrichment.automatic.value)
                assertTrue(enrichment.unmeteredOnly.value)
                assertFalse(requireNotNull(enrichment.status.value).running)
            } finally {
                runBlocking {
                    enrichment.setAutomatic(automatic).join()
                    enrichment.setUnmeteredOnly(unmetered).join()
                }
                features.setOnline(online)
                eventually { features.online.value == online }
            }
        }
    }

    private fun eventually(condition: () -> Boolean) {
        val deadline = System.nanoTime() + 15_000_000_000L
        while (!condition()) {
            if (System.nanoTime() > deadline) throw AssertionError("Artwork state did not settle")
            Thread.sleep(100)
        }
    }
}
