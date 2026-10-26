package org.resonate.android.ui.equaliser

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.input.pointer.positionChange
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextMeasurer
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.drawText
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.unit.dp
import kotlin.math.abs
import kotlin.math.roundToInt
import org.resonate.android.nativecore.EqualiserCurve

private class Hold(val band: Int?, val span: Float, val start: Offset, val offset: Offset)

@Composable
fun EqualiserGraph(
    curve: EqualiserCurve,
    response: List<Float>,
    bounds: EqualiserBounds,
    selected: Int?,
    onSelect: (Int) -> Unit,
    onEdit: (EqualiserCurve) -> Unit,
    onCommit: (EqualiserCurve) -> Unit,
    onAdd: (hertz: Float, gainDb: Float) -> Unit,
    onRelease: () -> Unit,
    modifier: Modifier = Modifier,
) {
    var frozen by remember { mutableStateOf<Float?>(null) }
    val span = frozen ?: GraphScale.span(response, curve)
    val latest by rememberUpdatedState(curve)
    val latestSpan by rememberUpdatedState(span)
    val select by rememberUpdatedState(onSelect)
    val edit by rememberUpdatedState(onEdit)
    val commit by rememberUpdatedState(onCommit)
    val add by rememberUpdatedState(onAdd)
    val release by rememberUpdatedState(onRelease)
    val scheme = MaterialTheme.colorScheme
    val measurer = rememberTextMeasurer()
    val labels = MaterialTheme.typography.labelSmall.copy(color = scheme.onSurfaceVariant)
    val numbers = MaterialTheme.typography.labelSmall.copy(fontWeight = FontWeight.Bold)
    val density = LocalDensity.current
    val reach = with(density) { HANDLE_REACH.toPx() }
    val tapTravel = with(density) { TAP_TRAVEL.toPx() }
    val radius = with(density) { HANDLE_RADIUS.toPx() }
    Canvas(
        modifier
            .fillMaxWidth()
            .height(GRAPH_HEIGHT)
            .testTag("equaliser-graph")
            .semantics {
                contentDescription =
                    "Equaliser response, ${curve.bands.size} bands. Choose a band below to edit it."
            }
            .pointerInput(bounds) {
                awaitEachGesture {
                    val down = awaitFirstDown(requireUnconsumed = false)
                    val frame = GraphFrame(size.width.toFloat(), size.height.toFloat(), latestSpan)
                    val point = GraphPoint(down.position.x, down.position.y)
                    val found = frame.nearest(latest, point, reach)
                    val hold =
                        if (found == null) Hold(null, latestSpan, down.position, Offset.Zero)
                        else {
                            val centre = frame.handle(latest, found)
                            down.consume()
                            select(found)
                            Hold(found, latestSpan, down.position, Offset(centre.x, centre.y) - down.position)
                        }
                    frozen = hold.span
                    var moved = false
                    var abandoned = false
                    var finished = false
                    var last = down.position
                    var placed: EqualiserCurve? = null
                    try {
                        while (true) {
                            val event = awaitPointerEvent()
                            val change = event.changes.firstOrNull { it.id == down.id } ?: break
                            if (!change.pressed) {
                                last = change.position
                                break
                            }
                            if (hold.band != null) {
                                last = change.position
                                if (!moved && (change.position - hold.start).getDistance() > 2f) moved = true
                                if (moved) {
                                    val at = change.position + hold.offset
                                    val next = frame.placed(latest, hold.band, GraphPoint(at.x, at.y), bounds)
                                    placed = next
                                    edit(next)
                                }
                                change.consume()
                            } else {
                                last = change.position
                                if (change.isConsumed ||
                                    (change.positionChange() != Offset.Zero &&
                                        (change.position - hold.start).getDistance() > tapTravel)
                                ) {
                                    abandoned = true
                                    break
                                }
                            }
                        }
                        finished = true
                    } finally {
                        frozen = null
                        if (hold.band != null && moved) {
                            val ending = placed
                            if (finished && ending != null) commit(ending) else release()
                        }
                    }
                    if (hold.band == null && !abandoned && (last - hold.start).getDistance() <= tapTravel) {
                        val (hertz, gain) = frame.added(latest, GraphPoint(hold.start.x, hold.start.y))
                        add(hertz, gain)
                    }
                }
            }
    ) {
        drawGrid(span, scheme.outlineVariant, scheme.outline, measurer, labels)
        drawResponse(response, span, scheme.primary)
        curve.bands.indices.forEach { index ->
            val handle = GraphFrame(size.width, size.height, span).handle(curve, index)
            val centre = Offset(handle.x, handle.y)
            val chosen = index == selected
            drawCircle(scheme.surface, radius, centre)
            if (chosen) drawCircle(scheme.primary, radius, centre)
            drawCircle(scheme.primary, radius, centre, style = Stroke(2.dp.toPx()))
            val text = measurer.measure("${index + 1}", numbers.copy(color = if (chosen) scheme.onPrimary else scheme.primary))
            drawText(text, topLeft = centre - Offset(text.size.width / 2f, text.size.height / 2f))
        }
    }
}

private fun DrawScope.drawGrid(
    span: Float,
    faint: Color,
    marked: Color,
    measurer: TextMeasurer,
    labels: TextStyle,
) {
    val thin = 1.dp.toPx() / 2
    GraphScale.faintHertz.forEach { hertz ->
        val x = GraphScale.x(hertz, size.width)
        drawLine(faint.copy(alpha = 0.5f), Offset(x, 0f), Offset(x, size.height), thin)
    }
    GraphScale.labelledHertz.forEach { (hertz, label) ->
        val x = GraphScale.x(hertz, size.width)
        drawLine(faint, Offset(x, 0f), Offset(x, size.height), thin * 1.5f)
        val text = measurer.measure(label, labels)
        drawText(text, topLeft = Offset(x + 4.dp.toPx(), size.height - text.size.height - 2.dp.toPx()))
    }
    GraphScale.marks(span).forEach { level ->
        val y = GraphScale.y(level, size.height, span)
        val zero = abs(level) < 0.001f
        drawLine(if (zero) marked else faint.copy(alpha = 0.5f), Offset(0f, y), Offset(size.width, y), if (zero) thin * 2 else thin)
        if (zero || abs(level - span) < 0.001f || abs(level + span) < 0.001f) {
            val text = measurer.measure(if (zero) "0 dB" else level.roundToInt().let { if (it > 0) "+$it" else "$it" }, labels)
            val top = if (level >= span - 0.001f) y + 2.dp.toPx() else y - text.size.height - 2.dp.toPx()
            drawText(text, topLeft = Offset(4.dp.toPx(), top))
        }
    }
}

private fun DrawScope.drawResponse(response: List<Float>, span: Float, line: Color) {
    if (response.size < 2) return
    val trace = Path()
    response.forEachIndexed { index, decibels ->
        val x = GraphScale.responseX(index, response.size, size.width)
        val y = GraphScale.y(decibels, size.height, span)
        if (index == 0) trace.moveTo(x, y) else trace.lineTo(x, y)
    }
    val zero = GraphScale.y(0f, size.height, span)
    val wash =
        Path().apply {
            addPath(trace)
            lineTo(size.width, zero)
            lineTo(0f, zero)
            close()
        }
    drawPath(
        wash,
        Brush.verticalGradient(
            0f to line.copy(alpha = 0.28f),
            0.5f to line.copy(alpha = 0.08f),
            1f to line.copy(alpha = 0.28f),
            startY = 0f,
            endY = size.height,
        ),
    )
    drawPath(
        trace,
        line,
        style = Stroke(width = 2.5.dp.toPx(), cap = StrokeCap.Round, join = StrokeJoin.Round),
    )
}

private val GRAPH_HEIGHT = 240.dp
private val HANDLE_RADIUS = 13.dp
private val HANDLE_REACH = 32.dp
private val TAP_TRAVEL = 8.dp
