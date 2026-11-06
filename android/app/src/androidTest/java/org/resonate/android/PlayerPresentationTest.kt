package org.resonate.android

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.width
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.SnackbarHostState
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.v2.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.dp
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.After
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.resonate.android.ui.LocalResonate
import org.resonate.android.ui.components.LocalSheets
import org.resonate.android.ui.components.Sheets
import org.resonate.android.ui.components.StatusNotice
import org.resonate.android.ui.player.PlayerActions
import org.resonate.android.ui.player.PlayerViewModel

@RunWith(AndroidJUnit4::class)
class PlayerPresentationTest {
    @get:Rule val rule = createComposeRule()

    private val resonate =
        (InstrumentationRegistry.getInstrumentation().targetContext.applicationContext
                as ResonateApplication)
            .resonate

    @After
    fun dismissTheNotice() {
        resonate.library.dismissStatus()
    }

    @Test
    fun aDeviceNoticeWrapsAndTimesOut() {
        resonate.library.notice(DEVICE_NOTICE)
        rule.setContent {
            CompositionLocalProvider(LocalResonate provides resonate) {
                MaterialTheme { Box(Modifier.width(280.dp)) { StatusNotice() } }
            }
        }
        val layout = layoutOf(DEVICE_NOTICE)
        assertTrue(layout.lineCount > 1)
        assertFalse(layout.hasVisualOverflow)
        rule.onNodeWithContentDescription("Dismiss").assertIsDisplayed()
        rule.waitUntil(timeoutMillis = 10_000) { resonate.library.status.value == null }
        rule.waitForIdle()
        rule.onNodeWithText(DEVICE_NOTICE).assertDoesNotExist()
    }

    @Test
    fun aDeviceNoticeCanBeDismissedBeforeItTimesOut() {
        resonate.library.notice(DEVICE_NOTICE)
        rule.setContent {
            CompositionLocalProvider(LocalResonate provides resonate) {
                MaterialTheme { StatusNotice() }
            }
        }
        rule.onNodeWithContentDescription("Dismiss").performClick()
        rule.onNodeWithText(DEVICE_NOTICE).assertDoesNotExist()
    }

    @Test
    fun theQueueLabelFitsAtNarrowWidthsAndLargeText() {
        val model = PlayerViewModel(resonate)
        val sheets = Sheets(SnackbarHostState())
        var fontScale by mutableStateOf(1f)
        rule.setContent {
            CompositionLocalProvider(
                LocalResonate provides resonate,
                LocalSheets provides sheets,
                LocalDensity provides Density(LocalDensity.current.density, fontScale),
            ) {
                MaterialTheme {
                    Box(Modifier.width(280.dp)) {
                        PlayerActions(model, false, null, {}, {}, {})
                    }
                }
            }
        }
        for (scale in listOf(1f, 1.3f, 2f)) {
            rule.runOnIdle { fontScale = scale }
            for (label in listOf("Lyrics", "Queue", "Zones")) {
                rule.onNodeWithText(label).assertIsDisplayed()
                val layout = layoutOf(label)
                assertFalse(
                    "$label is truncated at font scale $scale",
                    layout.isClipped() ||
                        (0 until layout.lineCount).any { layout.isLineEllipsized(it) },
                )
            }
        }
    }

    private fun layoutOf(text: String): TextLayoutResult {
        val layouts = mutableListOf<TextLayoutResult>()
        rule.onNodeWithText(text, useUnmergedTree = true).performSemanticsAction(
            SemanticsActions.GetTextLayoutResult
        ) {
            it(layouts)
        }
        return layouts.single()
    }

    private companion object {
        const val DEVICE_NOTICE = "The audio device stopped playing — playing on once one is there"
    }
}
