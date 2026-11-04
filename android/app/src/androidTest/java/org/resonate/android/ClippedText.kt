package org.resonate.android

import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.style.TextOverflow

private const val ROUNDING_PX = 1f

fun TextLayoutResult.isClipped(): Boolean {
    val cutShort = multiParagraph.didExceedMaxLines && layoutInput.overflow != TextOverflow.Ellipsis
    val widerThanItsBox =
        (0 until lineCount).any { line ->
            getLineRight(line) - getLineLeft(line) > size.width + ROUNDING_PX
        }

    return cutShort || widerThanItsBox
}
