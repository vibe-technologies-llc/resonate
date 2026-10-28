package org.resonate.android

import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.uiautomator.By
import androidx.test.uiautomator.BySelector
import androidx.test.uiautomator.Direction
import androidx.test.uiautomator.UiDevice
import androidx.test.uiautomator.UiObject2
import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class UiSmokeTest {
    private companion object {
        const val PAGE_SETTLES_MS = 600L
    }

    @Test
    fun settingsAndNativePreferencesSurviveActivityRecreation() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val device = UiDevice.getInstance(instrumentation)
        val library =
            (instrumentation.targetContext.applicationContext as ResonateApplication).library
        ActivityScenario.launch(MainActivity::class.java).use { scenario ->
            pressed(device, By.desc("Settings"))
            val online = library.features.online.value
            pressed(device, By.res("setting-online"))
            eventually { library.features.online.value != online }
            pressed(device, By.res("setting-online"))
            eventually { library.features.online.value == online }
            pressed(device, By.res("setting-processing"))
            await(device, By.res("true-peak"))
            scenario.recreate()
            await(device, By.res("true-peak"))
            device.pressBack()
            scrollTo(device, By.res("setting-online"))
            assertEquals(online, library.features.online.value)
        }
    }

    private fun pressed(device: UiDevice, selector: BySelector) {
        repeat(5) {
            try {
                scrollTo(device, selector).click()
                Thread.sleep(PAGE_SETTLES_MS)
                return
            } catch (stale: androidx.test.uiautomator.StaleObjectException) {
                device.waitForIdle()
            }
        }
        scrollTo(device, selector).click()
        Thread.sleep(PAGE_SETTLES_MS)
    }

    private fun scrollTo(device: UiDevice, selector: BySelector): UiObject2 {
        for (direction in listOf(Direction.DOWN, Direction.UP)) {
            repeat(8) {
                InstrumentationRegistry.getInstrumentation().uiAutomation.forgetTheCachedTree()
                device.findObject(selector)?.let {
                    val at = it.visibleCenter.y
                    if (at in device.displayHeight / 5..device.displayHeight * 3 / 4) {
                        Thread.sleep(PAGE_SETTLES_MS)
                        return device.findObject(selector) ?: it
                    }
                    val middle = device.displayWidth / 2
                    val from = if (at > device.displayHeight / 2) device.displayHeight * 2 / 3 else device.displayHeight / 3
                    val to = if (at > device.displayHeight / 2) device.displayHeight / 3 else device.displayHeight * 2 / 3
                    device.swipe(middle, from, middle, to, 20)
                    device.waitForIdle()
                    return@repeat
                }
                val middle = device.displayWidth / 2
                val high = device.displayHeight / 3
                val low = device.displayHeight * 2 / 3
                if (direction == Direction.DOWN) device.swipe(middle, low, middle, high, 20)
                else device.swipe(middle, high, middle, low, 20)
                device.waitForIdle()
            }
        }
        return await(device, selector)
    }

    private fun await(device: UiDevice, selector: BySelector): UiObject2 {
        var found: UiObject2? = null
        eventually {
            InstrumentationRegistry.getInstrumentation().uiAutomation.forgetTheCachedTree()
            found = device.findObject(selector)
            found != null
        }
        return requireNotNull(found)
    }

    private fun screen(): String {
        val ids = StringBuilder()
        UiDevice.getInstance(InstrumentationRegistry.getInstrumentation())
            .findObjects(By.res(java.util.regex.Pattern.compile(".+")))
            .forEach {
                try {
                    ids.append(it.resourceName).append(' ')
                } catch (stale: androidx.test.uiautomator.StaleObjectException) {
                    ids.append("? ")
                }
            }
        return ids.toString()
    }

    private fun eventually(condition: () -> Boolean) {
        val deadline = System.nanoTime() + 15_000_000_000L
        while (!condition()) {
            if (System.nanoTime() > deadline)
                throw AssertionError("Settings state did not settle: ${screen()}")
            Thread.sleep(100)
        }
    }
}
