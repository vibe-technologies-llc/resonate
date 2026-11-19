package org.resonate.android.ui.home

import android.text.format.DateFormat
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.layout.layout
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalLocale
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.TextUnit
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.isSpecified
import androidx.compose.ui.unit.sp
import java.time.LocalDate
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.TextStyle
import java.util.Locale
import org.resonate.android.R
import org.resonate.android.ui.components.Sizes
import org.resonate.android.ui.components.rememberLargeText
import org.resonate.android.words

@Composable
fun ListeningGlanceCard(heard: ListeningGlance.Heard?, day: LocalDate?, wide: Boolean, onOpen: () -> Unit) {
    val words = words()
    val locale = LocalLocale.current.platformLocale
    val zone = ZoneId.systemDefault()
    val today = day ?: LocalDate.now(zone)
    val dated = remember(locale) { datedIn(locale) }
    val glance =
        remember(heard, words, today, locale) {
            if (heard == null) ListeningGlance.unread(words, today, zone, dated)
            else ListeningGlance.of(heard, words, today, zone, dated)
        }
    val spoken = if (heard == null) stringResource(R.string.glance_reading) else glance.spoken
    val openLabel = stringResource(R.string.glance_open)
    val arrangement = GlanceArrangement.of(wide, rememberLargeText())
    val drawsChart = glance.standing != ListeningGlance.Standing.RESTING_THIS_WEEK
    Box(
        Modifier.fillMaxWidth()
            .padding(horizontal = Sizes.gutter, vertical = 4.dp)
            .clip(MaterialTheme.shapes.large)
            .background(MaterialTheme.colorScheme.surfaceContainerHigh)
            .clickable(onClickLabel = openLabel, role = Role.Button, onClick = onOpen)
            .semantics { contentDescription = spoken }
            .testTag("home-glance")
    ) {
        Box(
            Modifier.fillMaxWidth()
                .padding(16.dp)
                .alpha(if (heard == null) UNREAD_ALPHA else 1f)
                .clearAndSetSemantics {}
        ) {
            when (arrangement) {
                GlanceArrangement.STACKED ->
                    Column(verticalArrangement = Arrangement.spacedBy(14.dp)) {
                        GlanceHeadline(glance)
                        if (drawsChart) WeekChart(glance, locale)
                        GlanceFacts(glance.facts)
                    }
                GlanceArrangement.BESIDE ->
                    Row(horizontalArrangement = Arrangement.spacedBy(24.dp)) {
                        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(14.dp)) {
                            GlanceHeadline(glance)
                            GlanceFacts(glance.facts)
                        }
                        if (drawsChart) Box(Modifier.weight(1f)) { WeekChart(glance, locale) }
                    }
            }
        }
    }
}

private fun datedIn(locale: Locale): Dated {
    val thisYear = DateTimeFormatter.ofPattern(DateFormat.getBestDateTimePattern(locale, DAY_AND_MONTH), locale)
    val anotherYear = DateTimeFormatter.ofPattern(DateFormat.getBestDateTimePattern(locale, DAY_MONTH_AND_YEAR), locale)
    return Dated { day, withYear -> (if (withYear) anotherYear else thisYear).format(day) }
}

@Composable
private fun GlanceHeadline(glance: ListeningGlance) {
    val scheme = MaterialTheme.colorScheme
    Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
        Text(glance.eyebrow, style = MaterialTheme.typography.titleSmall, color = scheme.onSurfaceVariant)
        Text(
            glance.headline,
            style =
                if (glance.standing == ListeningGlance.Standing.NOTHING_HEARD) MaterialTheme.typography.titleLarge
                else MaterialTheme.typography.displaySmall,
            fontWeight = FontWeight.Bold,
        )
        Text(glance.detail, style = MaterialTheme.typography.bodyMedium, color = scheme.onSurfaceVariant)
    }
}

@Composable
private fun WeekChart(glance: ListeningGlance, locale: Locale) {
    val scheme = MaterialTheme.colorScheme
    val density = LocalDensity.current
    val tallest = minutes(glance.tallestMs)
    val domain = maxOf(tallest, LOWEST_STUB_MINUTES)
    val stub = maxOf(tallest * STUB_OF_THE_TALLEST, LOWEST_STUB_MINUTES * STUB_OF_THE_TALLEST)
    val average = minutes(glance.averageMs)
    val worn = scheme.primary
    val resting = scheme.onSurfaceVariant.copy(alpha = RESTING_STUB_ALPHA)
    val rule = scheme.onSurfaceVariant
    val letters = cappedSize(MaterialTheme.typography.labelSmall.fontSize, density.fontScale)
    val height = CHART_HEIGHT
    Column(Modifier.fillMaxWidth().testTag("glance-chart")) {
        Row(Modifier.fillMaxWidth()) {
            Canvas(Modifier.weight(1f).height(height)) {
                val slot = size.width / glance.week.size
                val width = slot * BAR_WIDTH_OF_A_DAY
                val corner = CornerRadius(BAR_CORNER.toPx(), BAR_CORNER.toPx())
                glance.week.forEachIndexed { index, day ->
                    val shown = maxOf(minutes(day.listenedMs), stub)
                    val barHeight = (shown / domain).toFloat() * size.height
                    val fill =
                        when {
                            day.listenedMs == 0uL -> resting
                            day.isToday -> worn
                            else -> worn.copy(alpha = QUIET_BAR_ALPHA)
                        }
                    drawRoundRect(
                        fill,
                        Offset(index * slot + (slot - width) / 2f, size.height - barHeight),
                        Size(width, barHeight),
                        corner,
                    )
                }
                if (average > 0.0) {
                    val y = size.height - (average / domain).toFloat() * size.height
                    drawLine(
                        rule,
                        Offset(0f, y),
                        Offset(size.width, y),
                        strokeWidth = 1.dp.toPx(),
                        pathEffect = PathEffect.dashPathEffect(floatArrayOf(3.dp.toPx(), 3.dp.toPx())),
                    )
                }
            }
            if (average > 0.0) {
                val chartPx = with(density) { height.roundToPx() }
                val lineAt = (chartPx - average / domain * chartPx).toInt()
                Box(Modifier.width(AVERAGE_ROOM).height(height)) {
                    Text(
                        stringResource(R.string.glance_average),
                        fontSize = letters,
                        color = scheme.onSurfaceVariant,
                        maxLines = 1,
                        modifier =
                            Modifier.align(Alignment.TopEnd).layout { measurable, constraints ->
                                val placed = measurable.measure(constraints)
                                layout(placed.width, chartPx) {
                                    placed.place(0, (lineAt - placed.height / 2).coerceIn(0, chartPx - placed.height))
                                }
                            },
                    )
                }
            }
        }
        Spacer(Modifier.height(6.dp))
        Row(Modifier.fillMaxWidth().padding(end = if (average > 0.0) AVERAGE_ROOM else 0.dp)) {
            glance.week.forEach { day ->
                Text(
                    day.day.dayOfWeek.getDisplayName(TextStyle.NARROW, locale),
                    fontSize = letters,
                    fontWeight = if (day.isToday) FontWeight.Bold else FontWeight.Normal,
                    color = if (day.isToday) scheme.onSurface else scheme.onSurfaceVariant,
                    textAlign = TextAlign.Center,
                    modifier = Modifier.weight(1f),
                )
            }
        }
    }
}

@Composable
private fun GlanceFacts(facts: List<ListeningGlance.Fact>) {
    if (facts.isEmpty()) return
    BoxWithConstraints(Modifier.fillMaxWidth()) {
        val narrowest = NARROWEST_FACT * LocalDensity.current.fontScale.coerceAtLeast(1f)
        val columns = ((maxWidth + FACT_GAP) / (narrowest + FACT_GAP)).toInt().coerceIn(1, facts.size)
        Column(verticalArrangement = Arrangement.spacedBy(FACT_GAP)) {
            facts.chunked(columns).forEach { row ->
                Row(horizontalArrangement = Arrangement.spacedBy(FACT_GAP)) {
                    row.forEach { fact -> GlanceFact(fact, Modifier.weight(1f)) }
                    repeat(columns - row.size) { Spacer(Modifier.weight(1f)) }
                }
            }
        }
    }
}

@Composable
private fun GlanceFact(fact: ListeningGlance.Fact, modifier: Modifier) {
    Column(modifier.testTag("glance-fact"), verticalArrangement = Arrangement.spacedBy(1.dp)) {
        Text(fact.value, style = MaterialTheme.typography.titleSmall, fontWeight = FontWeight.SemiBold)
        Text(fact.caption, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

private fun cappedSize(size: TextUnit, fontScale: Float): TextUnit =
    if (size.isSpecified && fontScale > LETTERS_GROW_TO) (size.value * LETTERS_GROW_TO / fontScale).sp else size

private fun minutes(milliseconds: ULong): Double = milliseconds.toDouble() / MILLISECONDS_A_MINUTE

private val CHART_HEIGHT = 96.dp
private val BAR_CORNER = 4.dp
private val AVERAGE_ROOM = 36.dp
private val NARROWEST_FACT = 130.dp
private val FACT_GAP = 12.dp

private const val UNREAD_ALPHA = 0.4f
private const val MILLISECONDS_A_MINUTE = 60_000.0
private const val STUB_OF_THE_TALLEST = 0.02
private const val LOWEST_STUB_MINUTES = 1.0
private const val BAR_WIDTH_OF_A_DAY = 0.62f
private const val QUIET_BAR_ALPHA = 0.4f
private const val RESTING_STUB_ALPHA = 0.25f
private const val LETTERS_GROW_TO = 1.3f
private const val DAY_AND_MONTH = "MMMd"
private const val DAY_MONTH_AND_YEAR = "yMMMd"
