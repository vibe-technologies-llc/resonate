package org.resonate.android.ui.tools

import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.State
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.rotate
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.repeatOnLifecycle
import kotlin.math.PI
import kotlin.math.abs
import kotlin.math.max
import kotlin.math.cos
import kotlin.math.min
import kotlin.math.sin
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.FlowPreview
import kotlinx.coroutines.flow.sample
import kotlinx.coroutines.withContext
import org.resonate.android.ui.LocalResonate
import org.resonate.android.ui.components.Page
import org.resonate.android.ui.components.Sizes
import org.resonate.android.ui.shownArtist
import org.resonate.android.ui.shownTitle

private const val BAND_COUNT = 40
private const val WINDOW = 256
private const val CALM_EVERY_MS = 250L
private const val TURN_MS = 60_000
private const val SCOPE_FLOOR = 0.1f

private class Heard(val levels: List<Float>, val wave: List<Float>, val loudness: Float)

@Composable
fun VisualizerScreen() {
    val resonate = LocalResonate.current
    val snapshot by resonate.library.snapshot.collectAsStateWithLifecycle()
    val current = snapshot?.current
    Page("Visualizer") { padding ->
        BoxWithConstraints(Modifier.fillMaxSize().padding(padding), contentAlignment = Alignment.Center) {
            val wide = maxWidth > maxHeight
            val titles: @Composable () -> Unit = {
                Column(horizontalAlignment = Alignment.CenterHorizontally) {
                    Text(
                        current?.shownTitle ?: "Play a song to see it move",
                        style = MaterialTheme.typography.headlineSmall,
                        textAlign = TextAlign.Center,
                        maxLines = 2,
                        overflow = TextOverflow.Ellipsis,
                    )
                    Spacer(Modifier.height(4.dp))
                    Text(
                        current?.shownArtist ?: "Live audio from the Resonate engine",
                        style = MaterialTheme.typography.bodyLarge,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        textAlign = TextAlign.Center,
                        maxLines = 2,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
            }
            if (wide) {
                Row(
                    Modifier.fillMaxSize().padding(horizontal = Sizes.gutter),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(24.dp),
                ) {
                    Box(Modifier.weight(1f).fillMaxSize(), contentAlignment = Alignment.Center) { LiveVisual() }
                    Box(Modifier.weight(1f)) { titles() }
                }
            } else {
                Column(
                    Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(horizontal = Sizes.gutter),
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.Center,
                ) {
                    Box(Modifier.widthIn(max = 520.dp).fillMaxWidth(), contentAlignment = Alignment.Center) { LiveVisual() }
                    Spacer(Modifier.height(24.dp))
                    titles()
                }
            }
        }
    }
}

@OptIn(FlowPreview::class)
@Composable
private fun LiveVisual() {
    val resonate = LocalResonate.current
    val features = resonate.features
    val calm = rememberCalm()
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    var heard by remember { mutableStateOf(Heard(List(BAND_COUNT) { 0f }, emptyList(), 0f)) }
    LaunchedEffect(lifecycle, calm.reduced) {
        val bands = Bands(BAND_COUNT, WINDOW)
        lifecycle.repeatOnLifecycle(Lifecycle.State.STARTED) {
            features.visualize(true)
            try {
                val stream = if (calm.reduced) features.samples.sample(CALM_EVERY_MS) else features.samples
                stream.collect { samples ->
                    val next = withContext(Dispatchers.Default) { bands.levels(samples) }
                    val levels = if (calm.reduced) next else eased(heard.levels, next)
                    heard = Heard(levels, samples, loudnessOf(samples))
                }
            } finally {
                features.visualize(false)
            }
        }
    }
    val turn: State<Float>? =
        if (calm.reduced) null
        else
            rememberInfiniteTransition(label = "visualizer")
                .animateFloat(
                    0f,
                    360f,
                    infiniteRepeatable(tween(TURN_MS, easing = LinearEasing), RepeatMode.Restart),
                    label = "visualizer turn",
                )
    val scheme = MaterialTheme.colorScheme
    Column(horizontalAlignment = Alignment.CenterHorizontally) {
        Canvas(
            Modifier.fillMaxWidth()
                .aspectRatio(1f)
                .testTag("visualizer")
                .semantics { contentDescription = "Live visualisation of the music playing" }
        ) {
            if (calm.reduced) drawCalmBars(heard.levels, scheme.primary, scheme.tertiary, scheme.surfaceContainerHighest)
            else drawRing(heard, turn?.value ?: 0f, scheme.primary, scheme.tertiary, scheme.primaryContainer)
        }
        Spacer(Modifier.height(16.dp))
        Canvas(Modifier.fillMaxWidth().height(56.dp)) { drawWave(heard.wave, scheme.secondary) }
    }
}

private fun DrawScope.drawRing(heard: Heard, turn: Float, primary: Color, tertiary: Color, glow: Color) {
    val centre = Offset(size.width / 2, size.height / 2)
    val outer = min(size.width, size.height) / 2
    val inner = outer * 0.42f
    val swell = 1f + (heard.loudness * 2.2f).coerceAtMost(0.35f)
    drawCircle(
        Brush.radialGradient(
            listOf(glow.copy(alpha = 0.9f), glow.copy(alpha = 0.35f), Color.Transparent),
            center = centre,
            radius = inner * 1.6f * swell,
        ),
        radius = inner * 1.6f * swell,
        center = centre,
    )
    drawCircle(primary.copy(alpha = 0.18f), inner * swell * 0.92f, centre)
    val spokes = heard.levels.size * 2
    val stroke = (2 * PI * inner / spokes * 0.55f).toFloat()
    val brush = Brush.sweepGradient(listOf(primary, tertiary, primary, tertiary, primary), center = centre)
    rotate(turn, centre) {
        repeat(spokes) { spoke ->
            val band = if (spoke < heard.levels.size) spoke else spokes - 1 - spoke
            val level = heard.levels[band]
            val angle = 2 * PI * spoke / spokes - PI / 2
            val start = inner + stroke
            val end = start + (outer - start) * (0.04f + 0.96f * level)
            val direction = Offset(cos(angle).toFloat(), sin(angle).toFloat())
            drawLine(brush, centre + direction * start, centre + direction * end, stroke, StrokeCap.Round)
        }
    }
}

private fun DrawScope.drawCalmBars(levels: List<Float>, primary: Color, tertiary: Color, track: Color) {
    if (levels.isEmpty()) return
    val step = size.width / levels.size
    val bar = step * 0.6f
    val brush = Brush.verticalGradient(listOf(tertiary, primary))
    levels.forEachIndexed { index, level ->
        val x = index * step + step / 2
        drawLine(track, Offset(x, size.height - bar / 2), Offset(x, bar / 2), bar, StrokeCap.Round)
        val top = size.height - bar / 2 - (size.height - bar) * level
        drawLine(brush, Offset(x, size.height - bar / 2), Offset(x, top), bar, StrokeCap.Round)
    }
}

private fun DrawScope.drawWave(wave: List<Float>, line: Color) {
    val middle = size.height / 2
    if (wave.size < 2) {
        drawLine(line.copy(alpha = 0.4f), Offset(0f, middle), Offset(size.width, middle), 2.dp.toPx(), StrokeCap.Round)
        return
    }
    val path = Path()
    val gain = 1f / max(wave.maxOf { abs(it) }, SCOPE_FLOOR)
    wave.forEachIndexed { index, sample ->
        val x = index.toFloat() / (wave.size - 1) * size.width
        val y = middle - (sample * gain).coerceIn(-1f, 1f) * middle
        if (index == 0) path.moveTo(x, y) else path.lineTo(x, y)
    }
    drawPath(path, line, style = Stroke(2.dp.toPx(), cap = StrokeCap.Round))
}
