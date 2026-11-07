package org.resonate.android

import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.uiautomator.By
import androidx.test.uiautomator.BySelector
import androidx.test.uiautomator.Direction
import androidx.test.uiautomator.StaleObjectException
import androidx.test.uiautomator.UiDevice
import androidx.test.uiautomator.UiObject2
import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class UiSmokeTest {
    private companion object {
        const val PAGE_SETTLES_MS = 600L
        const val SCROLLED_LIST = "settings-list"
        const val SCROLLS_AT_MOST = 12
        const val SWIPE_NEAR_TWENTIETHS = 9
        const val SWIPE_FAR_TWENTIETHS = 17
        const val SWIPE_STEPS = 100
        const val CLEAR_FROM_TWENTIETHS = 6
        const val CLEAR_TO_TWENTIETHS = 17
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
            device.waitForIdle()
            Thread.sleep(PAGE_SETTLES_MS)
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
            } catch (stale: StaleObjectException) {
                device.waitForIdle()
            }
        }
        scrollTo(device, selector).click()
        Thread.sleep(PAGE_SETTLES_MS)
    }

    private fun scrollTo(device: UiDevice, selector: BySelector): UiObject2 {
        for (direction in listOf(Direction.DOWN, Direction.UP)) {
            repeat(SCROLLS_AT_MOST) {
                try {
                    InstrumentationRegistry.getInstrumentation().uiAutomation.forgetTheCachedTree()
                    val found = device.findObject(selector)
                    val list = device.findObject(By.res(SCROLLED_LIST))
                    val bounds = list?.visibleBounds
                    val clear = bounds?.let { clearBand(it) }
                    val at = found?.visibleCenter?.y
                    if (found != null && (clear == null || at in clear)) {
                        device.waitForIdle()
                        Thread.sleep(PAGE_SETTLES_MS)
                        return await(device, selector)
                    }
                    if (list == null || clear == null) return await(device, selector)
                    val towards =
                        when {
                            at == null -> direction
                            at < clear.first -> Direction.UP
                            else -> Direction.DOWN
                        }
                    swiped(device, list, towards)
                    device.waitForIdle()
                    Thread.sleep(PAGE_SETTLES_MS)
                } catch (stale: StaleObjectException) {
                    device.waitForIdle()
                }
            }
        }
        return await(device, selector)
    }

    private fun clearBand(bounds: android.graphics.Rect): IntRange {
        val from = bounds.top + bounds.height() * CLEAR_FROM_TWENTIETHS / 20
        val to = bounds.top + bounds.height() * CLEAR_TO_TWENTIETHS / 20
        return from..to
    }

    private fun swiped(device: UiDevice, list: UiObject2, direction: Direction) {
        val bounds = list.visibleBounds
        val across = bounds.centerX()
        val near = bounds.top + bounds.height() * SWIPE_NEAR_TWENTIETHS / 20
        val far = bounds.top + bounds.height() * SWIPE_FAR_TWENTIETHS / 20
        if (direction == Direction.DOWN) device.swipe(across, far, across, near, SWIPE_STEPS)
        else device.swipe(across, near, across, far, SWIPE_STEPS)
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
                } catch (stale: StaleObjectException) {
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
