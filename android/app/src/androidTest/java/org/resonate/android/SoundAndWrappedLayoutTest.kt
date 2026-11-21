package org.resonate.android

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.width
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsSelected
import androidx.compose.ui.test.getUnclippedBoundsInRoot
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.v2.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollToIndex
import androidx.compose.ui.unit.dp
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.resonate.android.nativecore.EqualiserBand
import org.resonate.android.nativecore.EqualiserCurve
import org.resonate.android.nativecore.EqualiserFilter
import org.resonate.android.nativecore.WrappedRow
import org.resonate.android.nativecore.WrappedSlide
import org.resonate.android.nativecore.WrappedYear
import org.resonate.android.ui.LocalResonate
import org.resonate.android.ui.equaliser.BandChips
import org.resonate.android.ui.equaliser.EqualiserBounds
import org.resonate.android.ui.equaliser.EqualiserCard
import org.resonate.android.ui.equaliser.OCTAVE_Q
import org.resonate.android.ui.equaliser.octaveCentres
import org.resonate.android.ui.settings.Choice
import org.resonate.android.ui.settings.ChoiceDialog
import org.resonate.android.ui.wrapped.WrappedDeck

@RunWith(AndroidJUnit4::class)
class SoundAndWrappedLayoutTest {
    @get:Rule val rule = createComposeRule()

    @Test
    fun scrollingBandsStayAlignedWithTheGraphAndLeaveAddInPlace() {
        val bands = octaveCentres.map { EqualiserBand(EqualiserFilter.PEAKING, it, 0f, OCTAVE_Q) }
        rule.setContent {
            MaterialTheme {
                Column(Modifier.width(320.dp)) {
                    EqualiserCard {
                        Box(Modifier.fillMaxWidth().height(40.dp).testTag("graph-edge"))
                    }
                    BandChips(EqualiserCurve("", 0f, bands), null, EqualiserBounds.unread, {}, {})
                }
            }
        }
        val graph = rule.onNodeWithTag("graph-edge").getUnclippedBoundsInRoot()
        val scroll = rule.onNodeWithTag("band-chips")
        val add = rule.onNodeWithTag("add-band")
        val addBounds = add.getUnclippedBoundsInRoot()
        assertEquals(graph.left, scroll.getUnclippedBoundsInRoot().left)
        assertEquals(graph.right, addBounds.right)

        scroll.performScrollToIndex(bands.lastIndex)
        rule.onNodeWithTag("band-chip-${bands.lastIndex}").assertIsDisplayed()
        assertEquals(graph.left, scroll.getUnclippedBoundsInRoot().left)
        assertEquals(addBounds, add.getUnclippedBoundsInRoot())
    }

    @Test
    fun aChoiceWithADescriptionIsCompactAndStillSelectsAndCloses() {
        var open by mutableStateOf(true)
        var chosen = 0
        rule.setContent {
            MaterialTheme {
                if (open) {
                    ChoiceDialog(
                        title = "Quality",
                        choices = listOf(Choice(0, "Balanced", "Less battery"), Choice(1, "High")),
                        selected = chosen,
                        onChoose = { chosen = it },
                        onDismiss = { open = false },
                    )
                }
            }
        }
        val radio = SemanticsMatcher.expectValue(SemanticsProperties.Role, Role.RadioButton)
        val option = rule.onNode(radio and hasText("Balanced"))
        option.assertIsSelected()
        val bounds = option.getUnclippedBoundsInRoot()
        val height = bounds.bottom - bounds.top
        assertTrue("Radio row height was $height", height in 48.dp..56.dp)
        rule.onNodeWithText("High").performClick()
        rule.onNodeWithText("Quality").assertDoesNotExist()
        rule.runOnIdle { assertEquals(1, chosen) }
    }

    @Test
    fun wrappedOpensWithTheSummaryInTheMiddleAndCanPageForward() {
        val resonate =
            (InstrumentationRegistry.getInstrumentation().targetContext.applicationContext
                    as ResonateApplication)
                .resonate
        val slides =
            listOf(
                WrappedSlide(
                    "Your year in music",
                    "292 hours of music",
                    "166 songs",
                    emptyList(),
                    emptyList(),
                ),
                WrappedSlide(
                    "Your top song",
                    "A song",
                    null,
                    listOf(WrappedRow("Another song", null, "10 plays")),
                    emptyList(),
                ),
            )
        rule.setContent {
            CompositionLocalProvider(LocalResonate provides resonate) {
                MaterialTheme {
                    WrappedDeck(WrappedYear(2026, listOf(2026), slides, "Summary"), {}, {})
                }
            }
        }
        val viewport = rule.onNodeWithTag("wrapped-slides").fetchSemanticsNode().boundsInRoot
        val headline =
            rule.onNodeWithText(slides.first().headline).fetchSemanticsNode().boundsInRoot
        val fraction = (headline.center.y - viewport.top) / viewport.height
        assertTrue("Summary headline was at $fraction of the slide", fraction in 0.35f..0.75f)

        rule.onNodeWithTag("wrapped-next").performClick()
        rule.onNodeWithText(slides.last().headline).assertIsDisplayed()
        rule.onNodeWithText("Another song").assertIsDisplayed()
    }
}
