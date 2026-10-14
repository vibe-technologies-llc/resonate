package org.resonate.android

import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.uiautomator.By
import androidx.test.uiautomator.BySelector
import androidx.test.uiautomator.UiDevice
import androidx.test.uiautomator.UiObject2
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class UiSmokeTest {
    @Test
    fun libraryToolsAndSettingsOpenWithoutNetworkAccounts() {
        val device = UiDevice.getInstance(InstrumentationRegistry.getInstrumentation())
        ActivityScenario.launch(MainActivity::class.java).use {
            await(device, By.text("Songs"))
            await(device, By.desc("Library tools")).click()
            await(device, By.text("Your library"))
            await(device, By.text("Statistics")).click()
            await(device, By.text("Listening statistics"))
            device.pressBack()
            await(device, By.text("Your library"))
            await(device, By.text("Settings")).click()
            await(device, By.desc("Search settings"))
            val destinations =
                listOf(
                    Triple("folders", "Music folders", "Music folders"),
                    Triple("output", "Audio output", "Audio output"),
                    Triple(
                        "providers",
                        "Download providers",
                        "Downloads are matched by recording codes, or by title, artist and duration. Choose the destination folder for each song.",
                    ),
                    Triple(
                        "scrobbling",
                        "Scrobbling",
                        "Completed listens are queued on this device and submitted when online features are enabled.",
                    ),
                    Triple("headphone", "Headphone correction", "AutoEq headphone correction"),
                )
            destinations.forEach { (query, title, heading) ->
                searchSettings(device, query)
                await(device, By.text(title)).click()
                await(device, By.text(heading))
                device.pressBack()
                await(device, By.desc("Search settings"))
            }
        }
    }

    @Test
    fun settingsSearchAndNavigationSurviveActivityRecreation() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val device = UiDevice.getInstance(instrumentation)
        val library =
            (instrumentation.targetContext.applicationContext as ResonateApplication).library
        ActivityScenario.launch(MainActivity::class.java).use { scenario ->
            await(device, By.text("Songs"))
            await(device, By.text("Settings")).click()
            await(device, By.desc("Search settings"))
            val initialOnline = library.features.online.value
            await(device, By.text("Online features")).click()
            await(
                device,
                By.text(
                    if (initialOnline) "Artwork, lyrics and music search are offline"
                    else "Artwork, lyrics and music search are enabled"
                ),
            )
            await(device, By.text("Online features")).click()
            await(
                device,
                By.text(
                    if (initialOnline) "Artwork, lyrics and music search are enabled"
                    else "Artwork, lyrics and music search are offline"
                ),
            )
            assertEquals(initialOnline, library.features.online.value)
            await(device, By.text("Equaliser")).click()
            await(device, By.text("Enable equaliser"))
            await(device, By.text("Preamp"))
            device.pressBack()
            searchSettings(device, "ReplayGain")
            eventually(device) { !device.hasObject(By.text("Equaliser")) }
            await(device, By.text("Sound processing")).click()
            await(device, By.text("True-peak clip prevention"))
            scenario.recreate()
            await(device, By.text("True-peak clip prevention"))
            device.pressBack()
            await(device, By.clazz("android.widget.EditText").text("ReplayGain"))
            searchSettings(device, "Sleep")
            await(device, By.text("Sleep timer")).click()
            await(device, By.text("End of song"))
            await(device, By.text("30 minutes"))
            await(device, By.text("Done")).click()
        }
    }

    @Test
    fun onlineDefaultsOnAndPreservesAnExplicitOfflineChoice() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val preferences =
            context.getSharedPreferences("library-features", android.content.Context.MODE_PRIVATE)
        val previous =
            if (preferences.contains("online")) preferences.getBoolean("online", true) else null
        val library = (context.applicationContext as ResonateApplication).library
        try {
            preferences.edit().remove("online").commit()
            assertTrue(LibraryFeatures(context, library).online.value)
            preferences.edit().putBoolean("online", false).commit()
            assertEquals(false, LibraryFeatures(context, library).online.value)
        } finally {
            val edit = preferences.edit()
            if (previous == null) edit.remove("online") else edit.putBoolean("online", previous)
            edit.commit()
        }
    }

    private fun searchSettings(device: UiDevice, query: String) {
        await(device, By.clazz("android.widget.EditText")).setText(query)
        await(device, By.clazz("android.widget.EditText").text(query))
    }

    private fun await(device: UiDevice, selector: BySelector): UiObject2 {
        var result: UiObject2? = null
        eventually(device) {
            result = device.findObject(selector)
            result != null
        }
        return requireNotNull(result)
    }

    private fun eventually(device: UiDevice, condition: () -> Boolean) {
        val deadline = System.nanoTime() + 10_000_000_000L
        while (true) {
            InstrumentationRegistry.getInstrumentation().uiAutomation.clearCache()
            if (condition()) return
            if (System.nanoTime() > deadline) {
                val hierarchy =
                    java.io.ByteArrayOutputStream().use { output ->
                        device.dumpWindowHierarchy(output)
                        output.toString()
                    }
                throw AssertionError("UI did not settle: $hierarchy")
            }
            Thread.sleep(100)
        }
    }
}
