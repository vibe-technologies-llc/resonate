package org.resonate.android

import androidx.compose.foundation.layout.width
import androidx.compose.material3.Text
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertLeftPositionInRootIsEqualTo
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performScrollToIndex
import androidx.compose.ui.unit.dp
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.resonate.android.ui.components.Sizes
import org.resonate.android.ui.library.Shelf

@RunWith(AndroidJUnit4::class)
class ShelfHeadTest {
    @get:Rule val rule = createComposeRule()

    private var albums by mutableStateOf(HELD)

    private fun shelved() {
        rule.setContent {
            Shelf(albums, key = { it }, Modifier.testTag(SHELF)) { album ->
                Text(album, Modifier.width(CARD))
            }
        }
        rule.waitForIdle()
    }

    @Test
    fun anAlbumArrivingAtTheHeadOfAShelfShownFromItsStartIsInView() {
        shelved()

        rule.runOnIdle { albums = listOf(ARRIVED) + HELD }

        rule.onNodeWithText(ARRIVED).assertLeftPositionInRootIsEqualTo(Sizes.gutter)
        rule.onNodeWithText(HELD.first()).assertIsDisplayed()
    }

    @Test
    fun aShelfScrolledAlongStaysOnTheAlbumsItShowsWhenOneArrivesAtItsHead() {
        shelved()
        rule.onNodeWithTag(SHELF).performScrollToIndex(SCROLLED_TO)

        rule.runOnIdle { albums = listOf(ARRIVED) + HELD }

        rule.onNodeWithText(HELD[SCROLLED_TO]).assertIsDisplayed()
        rule.onNodeWithText(ARRIVED).assertDoesNotExist()
    }

    private companion object {
        const val SHELF = "shelf"
        const val ARRIVED = "Paper Tide"
        const val SCROLLED_TO = 6
        val HELD = (1..12).map { "Album $it" }
        val CARD = 160.dp
    }
}
