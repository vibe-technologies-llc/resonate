package org.resonate.android.ui.equaliser

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.size
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
import androidx.compose.ui.graphics.drawscope.inset
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.customActions
import androidx.compose.ui.semantics.onClick
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.TextMeasurer
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.drawText
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import kotlin.math.abs
import kotlin.math.roundToInt
import org.resonate.android.nativecore.EqualiserBand
import org.resonate.android.nativecore.EqualiserCurve

private val HANDLE_RADIUS = 12.dp
private val HANDLE_HALO = 20.dp
private val HANDLE_TARGET = 48.dp
private val GRAPH_INSET = 14.dp
private val STILL_WITHIN = 2.dp
private val LABEL_GAP = 8.dp
private const val HALO_ALPHA = 0.18f
private val WIDEST_LEVELS = listOf("+30", "\u221230")

@Composable
fun EqualiserGraph(
    curve: EqualiserCurve,
    response: List<Float>,
    bounds: EqualiserBounds,
    selected: Int?,
    onSelect: (Int) -> Unit,
    onEdit: (EqualiserCurve) -> Unit,
    onCommit: (EqualiserCurve) -> Unit,
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
    val release by rememberUpdatedState(onRelease)
    val scheme = MaterialTheme.colorScheme
    val measurer = rememberTextMeasurer()
    val labels = MaterialTheme.typography.labelSmall.copy(color = scheme.onSurfaceVariant)
    val numbers = MaterialTheme.typography.labelSmall.copy(fontWeight = FontWeight.Bold)
    val density = LocalDensity.current
    val reach = with(density) { (HANDLE_TARGET / 2).toPx() }
    val still = with(density) { STILL_WITHIN.toPx() }
    val inset = with(density) { GRAPH_INSET.toPx() }
    val target = with(density) { HANDLE_TARGET.toPx() }
    val gutter =
        remember(labels, density) {
            WIDEST_LEVELS.maxOf { measurer.measure(it, labels).size.width } +
                with(density) { LABEL_GAP.toPx() }
        }
    val chosenIndex = selected
    BoxWithConstraints(modifier.fillMaxWidth()) {
        val height = graphHeightDp(maxWidth.value).dp
        val frame =
            GraphFrame(
                constraints.maxWidth.toFloat(),
                with(density) { height.toPx() },
                span,
                inset,
                gutter,
            )
        Canvas(
            Modifier.fillMaxWidth()
                .height(height)
                .testTag("equaliser-graph")
                .semantics { contentDescription = graphDescription(curve, chosenIndex) }
                .pointerInput(bounds, inset, gutter) {
                    awaitEachGesture {
                        val down = awaitFirstDown(requireUnconsumed = false)
                        val grip =
                            GraphFrame(
                                size.width.toFloat(),
                                size.height.toFloat(),
                                latestSpan,
                                inset,
                                gutter,
                            )
                        val band =
                            grip.held(latest, GraphPoint(down.position.x, down.position.y), reach)
                                ?: return@awaitEachGesture
                        val centre = grip.handle(latest, band)
                        val offset = Offset(centre.x, centre.y) - down.position
                        down.consume()
                        select(band)
                        frozen = grip.span
                        var placed: EqualiserCurve? = null
                        var finished = false
                        try {
                            while (true) {
                                val event = awaitPointerEvent()
                                val change = event.changes.firstOrNull { it.id == down.id } ?: break
                                if (!change.pressed) {
                                    finished = !change.isConsumed
                                    break
                                }
                                val moved = (change.position - down.position).getDistance() > still
                                if (placed != null || moved) {
                                    val at = change.position + offset
                                    val next =
                                        grip.placed(latest, band, GraphPoint(at.x, at.y), bounds)
                                    placed = next
                                    edit(next)
                                }
                                change.consume()
                            }
                        } finally {
                            frozen = null
                            val ending = placed
                            if (ending != null) {
                                if (finished) commit(ending) else release()
                            }
                        }
                    }
                }
        ) {
            inset(frame.left, inset, inset, inset) {
                drawGrid(span, scheme.outlineVariant, scheme.outline, measurer, labels)
                drawResponse(response, span, scheme.primary)
            }
            drawLevels(frame, measurer, labels)
            curve.bands.indices.forEach { index ->
                val handle = frame.handle(curve, index)
                val centre = Offset(handle.x, handle.y)
                val chosen = index == chosenIndex
                val radius = HANDLE_RADIUS.toPx()
                if (chosen) {
                    drawCircle(scheme.primary.copy(alpha = HALO_ALPHA), HANDLE_HALO.toPx(), centre)
                }
                drawCircle(if (chosen) scheme.primary else scheme.surface, radius, centre)
                drawCircle(scheme.primary, radius, centre, style = Stroke(2.dp.toPx()))
                val text =
                    measurer.measure(
                        "${index + 1}",
                        numbers.copy(color = if (chosen) scheme.onPrimary else scheme.primary),
                    )
                drawText(
                    text,
                    topLeft = centre - Offset(text.size.width / 2f, text.size.height / 2f),
                )
            }
        }
        curve.bands.forEachIndexed { index, band ->
            val handle = frame.handle(curve, index)
            Box(
                Modifier.offset {
                        IntOffset(
                            (handle.x - target / 2).roundToInt(),
                            (handle.y - target / 2).roundToInt(),
                        )
                    }
                    .size(HANDLE_TARGET)
                    .testTag("band-handle-$index")
                    .semantics {
                        contentDescription = "Band ${index + 1}"
                        stateDescription = handleState(band)
                        this.selected = index == chosenIndex
                        onClick(label = "Edit band ${index + 1}") {
                            select(index)
                            true
                        }
                        customActions = handleActions(latest, index, bounds, commit, select)
                    }
            )
        }
    }
}

fun handleState(band: EqualiserBand): String {
    val gain = if (band.filter.usesGain) ", ${signedDb(band.gainDb)}" else ""
    return "${band.filter.label}, ${hertzLabel(band.frequencyHz)}$gain"
}

private fun handleActions(
    curve: EqualiserCurve,
    index: Int,
    bounds: EqualiserBounds,
    commit: (EqualiserCurve) -> Unit,
    select: (Int) -> Unit,
): List<CustomAccessibilityAction> {
    val band = curve.bands.getOrNull(index) ?: return emptyList()
    val edit: (EqualiserCurve) -> Boolean = { next ->
        select(index)
        commit(next)
        true
    }
    val gains =
        if (band.filter.usesGain)
            listOf(
                CustomAccessibilityAction("Raise the gain") {
                    edit(curve.withNudgedGain(index, GAIN_STEP_DB, bounds))
                },
                CustomAccessibilityAction("Lower the gain") {
                    edit(curve.withNudgedGain(index, -GAIN_STEP_DB, bounds))
                },
            )
        else emptyList()
    return gains +
        listOf(
            CustomAccessibilityAction("Raise the frequency") {
                edit(curve.withNudgedHertz(index, HERTZ_STEP_OCTAVES, bounds))
            },
            CustomAccessibilityAction("Lower the frequency") {
                edit(curve.withNudgedHertz(index, -HERTZ_STEP_OCTAVES, bounds))
            },
        )
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
        drawText(
            text,
            topLeft = Offset(x + 4.dp.toPx(), size.height - text.size.height - 2.dp.toPx()),
        )
    }
    GraphScale.marks(span).forEach { level ->
        val y = GraphScale.y(level, size.height, span)
        val zero = abs(level) < 0.001f
        drawLine(
            if (zero) marked else faint.copy(alpha = 0.5f),
            Offset(0f, y),
            Offset(size.width, y),
            if (zero) thin * 2 else thin,
        )
    }
}

private fun DrawScope.drawLevels(frame: GraphFrame, measurer: TextMeasurer, labels: TextStyle) {
    GraphScale.marks(frame.span)
        .filter { abs(it) < 0.001f || abs(abs(it) - frame.span) < 0.001f }
        .forEach { level ->
            val text = measurer.measure(levelLabel(level), labels)
            val y = frame.inset + GraphScale.y(level, frame.plotHeight, frame.span)
            val right = frame.left - LABEL_GAP.toPx() / 2
            drawText(
                text,
                topLeft = Offset(right - text.size.width, y - text.size.height / 2f),
            )
        }
}

fun levelLabel(level: Float): String =
    level.roundToInt().let {
        when {
            it > 0 -> "+$it"
            it < 0 -> "\u2212${-it}"
            else -> "0"
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

fun graphDescription(curve: EqualiserCurve, selected: Int?): String {
    val summary = "Equaliser response, ${curve.bands.size} bands."
    val band = selected?.let { curve.bands.getOrNull(it) }
    if (selected == null || band == null) return "$summary Choose a band to edit it."
    val place = "Band ${selected + 1} of ${curve.bands.size} selected, ${hertzLabel(band.frequencyHz)}"
    val gain = if (band.filter.usesGain) ", ${signedDb(band.gainDb)}" else ""
    return "$summary $place$gain."
}
