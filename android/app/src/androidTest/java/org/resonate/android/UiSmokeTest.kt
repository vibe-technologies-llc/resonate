package org.resonate.android

import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.uiautomator.By
import androidx.test.uiautomator.BySelector
import androidx.test.uiautomator.UiDevice
import androidx.test.uiautomator.UiObject2
import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class UiSmokeTest {
    @Test
    fun flutterSettingsAndNativePreferencesSurviveActivityRecreation() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val device = UiDevice.getInstance(instrumentation)
        val library =
            (instrumentation.targetContext.applicationContext as ResonateApplication).library
        ActivityScenario.launch(MainActivity::class.java).use { scenario ->
            await(device, By.desc("Your library"))
            await(device, By.descContains("Settings")).click()
            val online = library.features.online.value
            await(device, By.descContains("Online features")).click()
            eventually { library.features.online.value != online }
            await(device, By.descContains("Online features")).click()
            eventually { library.features.online.value == online }
            await(device, By.descContains("Sound processing")).click()
            await(device, By.descContains("True-peak clip prevention"))
            scenario.recreate()
            await(device, By.descContains("True-peak clip prevention"))
            device.pressBack()
            await(device, By.descContains("Online features"))
            assertEquals(online, library.features.online.value)
        }
    }

    private fun await(device: UiDevice, selector: BySelector): UiObject2 {
        var found: UiObject2? = null
        eventually {
            InstrumentationRegistry.getInstrumentation().uiAutomation.clearCache()
            found = device.findObject(selector)
            found != null
        }
        return requireNotNull(found)
    }

    private fun eventually(condition: () -> Boolean) {
        val deadline = System.nanoTime() + 15_000_000_000L
        while (!condition()) {
            if (System.nanoTime() > deadline) throw AssertionError("Flutter state did not settle")
            Thread.sleep(100)
        }
    }
}
