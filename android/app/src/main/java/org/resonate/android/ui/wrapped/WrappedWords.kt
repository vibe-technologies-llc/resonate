package org.resonate.android.ui.wrapped

private const val WHOLE_SHARE = 1_000f
private const val EVERY_LABEL_AT_MOST = 7
private const val EVERY_THIRD_LABEL_AT_MOST = 12
private const val EVERY_SIXTH = 6
private const val EVERY_THIRD = 3

object WrappedWords {
    fun entry(year: Long): String = "Your $year in music"

    fun place(slide: Int, slides: Int): String = "${slide + 1} of $slides"

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
