package org.resonate.android.ui.settings

import java.text.NumberFormat
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import java.util.Locale
import org.resonate.android.nativecore.LibraryShare
import org.resonate.android.nativecore.ListenedOnDay

enum class Period(val days: UInt, val label: String, val within: String) {
    ALL_TIME(0u, "All", "ever"),
    WEEK(7u, "Week", "this week"),
    MONTH(30u, "Month", "this month"),
    YEAR(365u, "Year", "this year"),
}

data class Figure(val value: String, val unit: String)

private const val MINUTE_MS = 60_000uL
private const val HOUR_MS = 3_600_000uL

fun grouped(count: ULong, locale: Locale = Locale.getDefault()): String =
    NumberFormat.getIntegerInstance(locale).format(count.toLong())

fun durationFigure(milliseconds: ULong, locale: Locale = Locale.getDefault()): Figure {
    val minutes = milliseconds / MINUTE_MS
    return when {
        milliseconds in 1uL until MINUTE_MS -> Figure("<1", "minute")
        milliseconds < HOUR_MS -> Figure(minutes.toString(), if (minutes == 1uL) "minute" else "minutes")
        milliseconds < 10uL * HOUR_MS -> {
            val tenths = (milliseconds * 10uL + HOUR_MS / 2uL) / HOUR_MS
            val whole = tenths / 10uL
            val rest = tenths % 10uL
            val separator =
                java.text.DecimalFormatSymbols.getInstance(locale).decimalSeparator
            Figure(if (rest == 0uL) "$whole" else "$whole$separator$rest", "hours")
        }
        else -> {
            val hours = (milliseconds + HOUR_MS / 2uL) / HOUR_MS
            Figure(grouped(hours, locale), "hours")
        }
    }
}

fun listenedLine(plays: ULong, listenedMs: ULong): String {
    val listens = "${grouped(plays)} ${if (plays == 1uL) "listen" else "listens"}"
    if (listenedMs == 0uL) return listens
    val figure = durationFigure(listenedMs)
    return "$listens · ${figure.value} ${figure.unit}"
}

fun shareOf(part: ULong, whole: ULong): Float =
    if (whole == 0uL) 0f else (part.toDouble() / whole.toDouble()).toFloat().coerceIn(0f, 1f)

fun percent(part: ULong, whole: ULong): String {
    if (whole == 0uL || part == 0uL) return "0%"
    val value = part.toDouble() * 100.0 / whole.toDouble()
    return if (value < 1.0) "<1%" else "${value.toInt().coerceAtMost(100)}%"
}

fun shareWhole(shares: List<LibraryShare>, songs: ULong): ULong =
    maxOf(songs, shares.sumOf { it.songs })

fun dayLabel(starts: ULong, zone: ZoneId = ZoneId.systemDefault(), locale: Locale = Locale.getDefault()): String =
    DateTimeFormatter.ofLocalizedDate(FormatStyle.MEDIUM)
        .withLocale(locale)
        .format(Instant.ofEpochSecond(starts.toLong()).atZone(zone))

fun busiestDay(days: List<ListenedOnDay>): ListenedOnDay? =
    days.filter { it.listenedMs > 0uL }.maxByOrNull { it.listenedMs }

fun daysShown(days: List<ListenedOnDay>, atMost: Int): List<ListenedOnDay> = days.takeLast(atMost)
