package org.resonate.android

import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.junit4.accessibility.enableAccessibilityChecks
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollToNode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.text.TextLayoutResult
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.After
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class A11ySweepTest {
    @get:Rule val rule = createAndroidComposeRule<MainActivity>()

    private val automation = InstrumentationRegistry.getInstrumentation().uiAutomation

    @Before
    fun checking() {
        rule.enableAccessibilityChecks()
    }

    @After
    fun restoringTheFontScale() {
        shell("settings put system font_scale 1.0")
    }

    @Test
    fun theRootScreensPassTheAccessibilityChecks() {
        ROOT_TABS.forEach { tab ->
            rule.onNodeWithTag(tab).performClick()
            rule.waitForIdle()
        }
    }

    @Test
    fun settingsAndAppearancePassTheAccessibilityChecks() {
        rule.onNodeWithContentDescription("Settings").performClick()
        rule.waitForIdle()
        rule.onNodeWithTag("settings-list").performScrollToNode(hasTestTag("setting-appearance"))
        rule.onNodeWithTag("setting-appearance").performClick()
        rule.waitForIdle()
        rule.onNodeWithTag("high-contrast").assertExists()
        rule.onNodeWithTag("pitch-black").assertExists()
    }

    @Test
    fun theLargestTextDoesNotClipAnyRootScreen() {
        shell("settings put system font_scale 2.0")
        rule.waitForIdle()
        ROOT_TABS.forEach { tab ->
            rule.onNodeWithTag(tab).performClick()
            rule.waitForIdle()
            val clipped = clippedTexts()
            assertTrue("$tab clips text without an ellipsis: $clipped", clipped.isEmpty())
        }
    }

    @Test
    fun everyButtonOnARootScreenIsNamed() {
        ROOT_TABS.forEach { tab ->
            rule.onNodeWithTag(tab).performClick()
            rule.waitForIdle()
            val unnamed =
                rule
                    .onRoot()
                    .fetchSemanticsNode()
                    .allNodes()
                    .filter { it.config.contains(SemanticsActions.OnClick) }
                    .filter { it.isUnnamed() }
            assertTrue("$tab has unnamed controls: ${unnamed.map { it.id }}", unnamed.isEmpty())
        }
    }

    private fun clippedTexts(): List<String> =
        rule
            .onRoot()
            .fetchSemanticsNode()
            .allNodes()
            .filter { it.config.contains(SemanticsActions.GetTextLayoutResult) }
            .filter { node ->
                val results = mutableListOf<TextLayoutResult>()
                node.config
                    .getOrNull(SemanticsActions.GetTextLayoutResult)
                    ?.action
                    ?.invoke(results)
                results.any { it.isClipped() }
            }
            .map { node ->
                node.config.getOrNull(SemanticsProperties.Text).orEmpty().joinToString(" ") { it.text }
            }

    private fun SemanticsNode.isUnnamed(): Boolean {
        val described = config.getOrNull(SemanticsProperties.ContentDescription)
        val texted = config.getOrNull(SemanticsProperties.Text)
        return described.isNullOrEmpty() && texted.isNullOrEmpty() && children.none { !it.isUnnamed() }
    }

    private fun SemanticsNode.allNodes(): List<SemanticsNode> =
        listOf(this) + children.flatMap { it.allNodes() }

    private fun shell(command: String) {
        automation.executeShellCommand(command).close()
    }

    private companion object {
        val ROOT_TABS = listOf("tab-home", "tab-library", "tab-search")
    }
}
