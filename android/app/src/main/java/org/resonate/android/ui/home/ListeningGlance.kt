package org.resonate.android.ui.home

import java.time.DayOfWeek
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId
import java.time.temporal.ChronoUnit
import kotlin.math.abs
import kotlin.math.roundToInt
import org.resonate.android.R
import org.resonate.android.Words
import org.resonate.android.nativecore.ListenedOnDay

object ListenedTime {
    private const val MINUTE_MS = 60_000uL
    private const val MINUTES_AN_HOUR = 60L
    private const val HOURS_WITH_MINUTES_BELOW = 100L

    fun spoken(words: Words, milliseconds: ULong): String {
        val minutes = (milliseconds / MINUTE_MS).toLong()
        val hours = minutes / MINUTES_AN_HOUR
        val rest = minutes % MINUTES_AN_HOUR
        val saidMinutes = words.plural(R.plurals.listening_minutes, rest.toInt(), rest)
        if (hours == 0L) return saidMinutes
        val saidHours = words.plural(R.plurals.listening_hours, hours.toInt(), hours)
        if (rest == 0L) return saidHours
        return words.string(R.string.listening_hours_and_minutes, saidHours, saidMinutes)
    }

    fun brief(words: Words, milliseconds: ULong): String {
        val minutes = (milliseconds / MINUTE_MS).toLong()
        if (minutes < MINUTES_AN_HOUR) return words.string(R.string.listening_brief_minutes, minutes)
        val hours = minutes / MINUTES_AN_HOUR
        if (hours < HOURS_WITH_MINUTES_BELOW)
            return words.string(R.string.listening_brief_hours_minutes, hours, minutes % MINUTES_AN_HOUR)
        return words.string(R.string.listening_brief_hours, hours)
    }
}

sealed interface ListeningTrend {
    data object Steady : ListeningTrend

    data class Up(val percent: Int) : ListeningTrend

    data class Down(val percent: Int) : ListeningTrend

    data object MoreThanDouble : ListeningTrend

    companion object {
        private const val STEADY_WITHIN_PERCENT = 5
        private const val DOUBLED_AT_PERCENT = 100

        fun of(now: ULong, before: ULong): ListeningTrend? {
            if (before == 0uL) return null
            val moved = (now.toDouble() - before.toDouble()) / before.toDouble() * 100.0
            val percent = moved.roundToInt()
            return when {
                abs(percent) < STEADY_WITHIN_PERCENT -> Steady
                percent > DOUBLED_AT_PERCENT -> MoreThanDouble
                percent > 0 -> Up(percent)
                else -> Down(-percent)
            }
        }
    }
}

fun ListeningTrend.words(words: Words): String =
    when (this) {
        ListeningTrend.Steady -> words.string(R.string.listening_trend_steady)
        is ListeningTrend.Up -> words.string(R.string.listening_trend_up, percent)
        is ListeningTrend.Down -> words.string(R.string.listening_trend_down, percent)
        ListeningTrend.MoreThanDouble -> words.string(R.string.listening_trend_more_than_double)
    }

enum class GlanceArrangement {
    STACKED,
    BESIDE;

    companion object {
        fun of(wide: Boolean, largeText: Boolean): GlanceArrangement =
            if (wide && !largeText) BESIDE else STACKED
    }
}

fun interface Dated {
    fun of(day: LocalDate, withYear: Boolean): String
}

data class ListeningGlance(
    val standing: Standing,
    val week: List<Day>,
    val averageMs: ULong,
    val eyebrow: String,
    val headline: String,
    val detail: String,
    val facts: List<Fact>,
    val spoken: String,
) {
    enum class Standing {
        NOTHING_HEARD,
        HEARD_THIS_WEEK,
        RESTING_THIS_WEEK,
    }

    enum class FactKind {
        RUN,
        WEEKDAY,
        ALL_TIME,
        SONGS_HEARD,
    }

    data class Fact(val kind: FactKind, val value: String, val caption: String, val spoken: String)

    data class Day(val day: LocalDate, val listenedMs: ULong, val isToday: Boolean)

    data class Heard(
        val days: List<ListenedOnDay>,
        val firstHeard: ULong?,
        val plays: ULong,
        val listenedMs: ULong,
        val songsHeard: ULong,
        val songs: ULong,
    )

    val tallestMs: ULong
        get() = week.maxOfOrNull { it.listenedMs } ?: 0uL

    companion object {
        const val DAYS_SHOWN = 7
        const val RUN_SHOWN_FROM = 2
        const val HABIT_SHOWN_FROM_DAYS_HEARD = 7

        private val unheard = Heard(emptyList(), null, 0uL, 0uL, 0uL, 0uL)

        fun unread(words: Words, today: LocalDate, zone: ZoneId, dated: Dated): ListeningGlance =
            of(unheard, words, today, zone, dated)

        fun of(heard: Heard, words: Words, today: LocalDate, zone: ZoneId, dated: Dated): ListeningGlance {
            val byDay = listenedByDay(heard.days, zone)
            val week = lastDays(today).map { day -> Day(day, byDay[day] ?: 0uL, day == today) }

            val thisWeekMs = week.sumOf { it.listenedMs }
            val weekBeforeMs = lastDays(today.minusDays(DAYS_SHOWN.toLong())).sumOf { byDay[it] ?: 0uL }
            val averageMs = thisWeekMs / daysCovered(heard.firstHeard, today, zone).toULong()
            val trend = ListeningTrend.of(thisWeekMs, weekBeforeMs)

            val run = runOfDays(byDay.keys, today)
            val weekday = busiestWeekday(byDay)
            val lastHeard = byDay.keys.maxOrNull()

            val thisWeeksName = words.string(R.string.listening_glance_this_week)
            if (byDay.isEmpty() && heard.plays == 0uL) {
                val headline = words.string(R.string.listening_glance_nothing_heard)
                val detail = words.string(R.string.listening_glance_nothing_heard_detail)
                return ListeningGlance(
                    standing = Standing.NOTHING_HEARD,
                    week = week,
                    averageMs = 0uL,
                    eyebrow = thisWeeksName,
                    headline = headline,
                    detail = detail,
                    facts = emptyList(),
                    spoken = sentenced(listOf(headline, detail)),
                )
            }

            if (thisWeekMs == 0uL) {
                val detail =
                    lastHeard?.let {
                        words.string(
                            R.string.listening_glance_resting_last_heard,
                            dated.of(it, it.year != today.year),
                        )
                    } ?: words.string(R.string.listening_glance_resting)
                val facts = listOfNotNull(weekdayFact(words, weekday), songsHeardFact(words, heard))
                val plays = heard.plays.toInt()
                return ListeningGlance(
                    standing = Standing.RESTING_THIS_WEEK,
                    week = week,
                    averageMs = 0uL,
                    eyebrow = words.string(R.string.listening_glance_all_time),
                    headline = ListenedTime.brief(words, heard.listenedMs),
                    detail = detail,
                    facts = facts,
                    spoken =
                        sentenced(
                            listOf(
                                words.plural(
                                    R.plurals.listening_glance_resting_spoken,
                                    plays,
                                    ListenedTime.spoken(words, heard.listenedMs),
                                    plays,
                                ),
                                detail.replace(SEPARATOR, SPOKEN_SEPARATOR),
                            ) + facts.map { it.spoken }
                        ),
                )
            }

            val pace = words.string(R.string.listening_glance_a_day, ListenedTime.brief(words, averageMs))
            val trendWords = trend?.words(words)
            val facts =
                listOfNotNull(
                    runFact(words, run),
                    weekdayFact(words, weekday),
                    allTimeFact(words, heard),
                    songsHeardFact(words, heard),
                )
            val spokenPace =
                listOfNotNull(
                        words.string(R.string.listening_glance_a_day, ListenedTime.spoken(words, averageMs)),
                        trendWords,
                    )
                    .joinToString(SPOKEN_SEPARATOR)
            val busiest =
                week.maxByOrNull { it.listenedMs }?.let { day ->
                    words.string(
                        R.string.listening_glance_busiest,
                        dayName(words, day.day.dayOfWeek),
                        ListenedTime.spoken(words, day.listenedMs),
                    )
                }
            return ListeningGlance(
                standing = Standing.HEARD_THIS_WEEK,
                week = week,
                averageMs = averageMs,
                eyebrow = thisWeeksName,
                headline = ListenedTime.brief(words, thisWeekMs),
                detail = listOfNotNull(pace, trendWords).joinToString(SEPARATOR),
                facts = facts,
                spoken =
                    sentenced(
                        listOfNotNull(
                            words.string(
                                R.string.listening_glance_this_week_spoken,
                                ListenedTime.spoken(words, thisWeekMs),
                            ),
                            spokenPace,
                            busiest,
                        ) + facts.map { it.spoken }
                    ),
            )
        }

        private const val SEPARATOR = " · "
        private const val SPOKEN_SEPARATOR = ", "
        private const val SENTENCE_SEPARATOR = ". "

        private fun sentenced(sentences: List<String>): String = sentences.joinToString(SENTENCE_SEPARATOR)

        private fun dayOf(starts: ULong, zone: ZoneId): LocalDate =
            Instant.ofEpochSecond(starts.toLong()).atZone(zone).toLocalDate()

        private fun listenedByDay(days: List<ListenedOnDay>, zone: ZoneId): Map<LocalDate, ULong> {
            val byDay = mutableMapOf<LocalDate, ULong>()
            for (day in days) {
                if (day.listenedMs == 0uL && day.plays == 0uL) continue
                val starts = dayOf(day.starts, zone)
                byDay[starts] = (byDay[starts] ?: 0uL) + day.listenedMs
            }
            return byDay
        }

        private fun lastDays(last: LocalDate): List<LocalDate> =
            (DAYS_SHOWN - 1 downTo 0).map { last.minusDays(it.toLong()) }

        private fun daysCovered(firstHeard: ULong?, today: LocalDate, zone: ZoneId): Int {
            if (firstHeard == null) return DAYS_SHOWN
            val since = ChronoUnit.DAYS.between(dayOf(firstHeard, zone), today).toInt()
            return (since + 1).coerceIn(1, DAYS_SHOWN)
        }

        private fun runOfDays(heard: Set<LocalDate>, today: LocalDate): Int {
            val yesterday = today.minusDays(1)
            var day =
                when {
                    today in heard -> today
                    yesterday in heard -> yesterday
                    else -> return 0
                }
            var run = 0
            while (day in heard) {
                run++
                day = day.minusDays(1)
            }
            return run
        }

        private fun sundayFirst(day: DayOfWeek): Int = day.value % DAYS_SHOWN

        private fun busiestWeekday(byDay: Map<LocalDate, ULong>): DayOfWeek? {
            if (byDay.size < HABIT_SHOWN_FROM_DAYS_HEARD) return null
            val byWeekday = mutableMapOf<DayOfWeek, ULong>()
            for ((day, listened) in byDay) {
                byWeekday[day.dayOfWeek] = (byWeekday[day.dayOfWeek] ?: 0uL) + listened
            }
            return byWeekday.entries
                .maxWithOrNull(compareBy<Map.Entry<DayOfWeek, ULong>> { it.value }.thenByDescending { sundayFirst(it.key) })
                ?.key
        }

        private fun dayName(words: Words, day: DayOfWeek): String =
            words.string(
                when (day) {
                    DayOfWeek.SUNDAY -> R.string.listening_weekday_sunday
                    DayOfWeek.MONDAY -> R.string.listening_weekday_monday
                    DayOfWeek.TUESDAY -> R.string.listening_weekday_tuesday
                    DayOfWeek.WEDNESDAY -> R.string.listening_weekday_wednesday
                    DayOfWeek.THURSDAY -> R.string.listening_weekday_thursday
                    DayOfWeek.FRIDAY -> R.string.listening_weekday_friday
                    DayOfWeek.SATURDAY -> R.string.listening_weekday_saturday
                }
            )

        private fun everyDayName(words: Words, day: DayOfWeek): String =
            words.string(
                when (day) {
                    DayOfWeek.SUNDAY -> R.string.listening_weekdays_sunday
                    DayOfWeek.MONDAY -> R.string.listening_weekdays_monday
                    DayOfWeek.TUESDAY -> R.string.listening_weekdays_tuesday
                    DayOfWeek.WEDNESDAY -> R.string.listening_weekdays_wednesday
                    DayOfWeek.THURSDAY -> R.string.listening_weekdays_thursday
                    DayOfWeek.FRIDAY -> R.string.listening_weekdays_friday
                    DayOfWeek.SATURDAY -> R.string.listening_weekdays_saturday
                }
            )

        private fun runFact(words: Words, run: Int): Fact? {
            if (run < RUN_SHOWN_FROM) return null
            return Fact(
                FactKind.RUN,
                words.plural(R.plurals.listening_fact_run, run, run),
                words.string(R.string.listening_fact_run_caption),
                words.plural(R.plurals.listening_fact_run_spoken, run, run),
            )
        }

        private fun weekdayFact(words: Words, weekday: DayOfWeek?): Fact? {
            if (weekday == null) return null
            val name = everyDayName(words, weekday)
            return Fact(
                FactKind.WEEKDAY,
                name,
                words.string(R.string.listening_fact_weekday_caption),
                words.string(R.string.listening_fact_weekday_spoken, name),
            )
        }

        private fun allTimeFact(words: Words, heard: Heard): Fact {
            val plays = heard.plays.toInt()
            return Fact(
                FactKind.ALL_TIME,
                ListenedTime.brief(words, heard.listenedMs),
                words.plural(R.plurals.listening_fact_all_time_caption, plays, plays),
                words.plural(
                    R.plurals.listening_fact_all_time_spoken,
                    plays,
                    ListenedTime.spoken(words, heard.listenedMs),
                    plays,
                ),
            )
        }

        private fun songsHeardFact(words: Words, heard: Heard): Fact {
            val reading =
                if (heard.songsHeard <= heard.songs)
                    words.string(
                        R.string.listening_fact_songs_heard_of,
                        heard.songsHeard.toLong(),
                        heard.songs.toLong(),
                    )
                else heard.songsHeard.toString()
            return Fact(
                FactKind.SONGS_HEARD,
                reading,
                words.string(R.string.listening_fact_songs_heard_caption),
                words.string(R.string.listening_fact_songs_heard_spoken, reading),
            )
        }
    }
}
