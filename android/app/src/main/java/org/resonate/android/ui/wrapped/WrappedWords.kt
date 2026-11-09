package org.resonate.android.ui.wrapped

import org.resonate.android.R
import org.resonate.android.Words

private const val WHOLE_SHARE = 1_000f
private const val EVERY_LABEL_AT_MOST = 7
private const val EVERY_THIRD_LABEL_AT_MOST = 12
private const val EVERY_SIXTH = 6
private const val EVERY_THIRD = 3

object WrappedWords {
    fun entry(words: Words, year: Long): String = words.string(R.string.wrapped_entry, year)

    fun place(words: Words, slide: Int, slides: Int): String =
        words.string(R.string.wrapped_place, slide + 1, slides)

    fun cardFileName(year: Long, slide: Int): String = "resonate-$year-${slide + 1}.png"

    fun barFraction(sharePermille: UShort): Float =
        (sharePermille.toInt() / WHOLE_SHARE).coerceIn(0f, 1f)

    fun labelsBar(at: Int, bars: Int): Boolean =
        when {
            bars <= EVERY_LABEL_AT_MOST -> true
            bars <= EVERY_THIRD_LABEL_AT_MOST -> at % EVERY_THIRD == 0
            else -> at % EVERY_SIXTH == 0
        }
}
