package org.resonate.android

import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.resonate.android.ui.Mix

@RunWith(AndroidJUnit4::class)
class HomeShortcutsTest {
    @get:Rule val rule = createAndroidComposeRule<MainActivity>()

    @Test
    fun everyBrowseShortcutIsVisibleOnHomeAndOpensItsCollection() {
        val tiles = hasTestTag("browse-tile") and hasAnyAncestor(hasTestTag("home"))
        rule.waitUntil(15_000) { rule.onAllNodes(tiles).fetchSemanticsNodes().size == 6 }

        val titles =
            listOf(
                R.string.browse_favourites,
                Mix.RECENTLY_ADDED.title,
                Mix.MOST_PLAYED.title,
                Mix.NEVER_HEARD.title,
                Mix.HIGH_RESOLUTION.title,
                R.string.settings_wanted_title,
            )
        titles.forEach { resource ->
            val title = rule.activity.getString(resource)
            val shortcut = rule.onNode(tiles and hasText(title))
            shortcut.assertIsDisplayed().performClick()
            shortcut.assertDoesNotExist()
            rule.onNodeWithText(title).assertIsDisplayed()
            rule
                .onNodeWithContentDescription(rule.activity.getString(R.string.page_back))
                .performClick()
        }
    }
}
