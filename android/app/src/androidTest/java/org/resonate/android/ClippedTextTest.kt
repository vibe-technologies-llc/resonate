package org.resonate.android

import androidx.compose.foundation.layout.width
import androidx.compose.material3.Text
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class ClippedTextTest {
    @get:Rule val rule = createComposeRule()

    private fun laidOut(
        overflow: TextOverflow,
        softWrap: Boolean,
        maxLines: Int,
        words: String = LONG,
    ): TextLayoutResult {
        var result: TextLayoutResult? = null
        rule.setContent {
            Text(
                words,
                modifier = Modifier.width(NARROW),
                overflow = overflow,
                softWrap = softWrap,
                maxLines = maxLines,
                onTextLayout = { result = it },
            )
        }
        rule.waitForIdle()
        return checkNotNull(result)
    }

    @Test
    fun aLineRunningPastItsBoxIsClipped() {
        assertTrue(laidOut(TextOverflow.Clip, softWrap = false, maxLines = 1).isClipped())
    }

    @Test
    fun linesCutOffWithNoEllipsisAreClipped() {
        assertTrue(laidOut(TextOverflow.Clip, softWrap = true, maxLines = 1).isClipped())
    }

    @Test
    fun anEllipsisIsNotClipping() {
        assertFalse(laidOut(TextOverflow.Ellipsis, softWrap = true, maxLines = 1).isClipped())
    }

    @Test
    fun wrappedLinesAreNotClipped() {
        assertFalse(
            laidOut(TextOverflow.Clip, softWrap = true, maxLines = Int.MAX_VALUE).isClipped()
        )
    }

    @Test
    fun aWordNarrowerThanItsBoxIsNotClipped() {
        assertFalse(
            laidOut(TextOverflow.Clip, softWrap = true, maxLines = 1, words = "Home").isClipped()
        )
    }

    private companion object {
        const val LONG = "A title much longer than the narrow box it has been given here"
        val NARROW = 96.dp
    }
}
