package org.resonate.android

import android.graphics.Bitmap
import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.FastOutSlowInEasing
import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.Spring
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.spring
import androidx.compose.animation.core.tween
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.State
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.draw.drawWithCache
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.core.graphics.get
import kotlin.math.cos
import kotlin.math.sin
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.resonate.android.nativecore.Track

@Immutable data class ArtworkTones(val primary: Color, val secondary: Color)

@Composable
fun rememberArtworkTones(track: Track?, library: LibraryRepository?): State<ArtworkTones> {
    val fallback =
        ArtworkTones(MaterialTheme.colorScheme.primary, MaterialTheme.colorScheme.tertiary)
    val generation = library?.features?.artworkGeneration?.collectAsState()?.value
    return produceState(fallback, track?.id, track?.artRevision, generation, fallback) {
        value = fallback
        if (track != null && library != null) {
            val bitmap = library.features.artwork(track)
            if (bitmap != null)
                value = withContext(Dispatchers.Default) { artworkTones(bitmap, fallback) }
        }
    }
}

internal fun artworkTones(bitmap: Bitmap, fallback: ArtworkTones): ArtworkTones {
    val channels = Array(2) { LongArray(4) }
    repeat(16) { y ->
        repeat(16) { x ->
            val pixel = bitmap[x * bitmap.width / 16, y * bitmap.height / 16]
            val red = android.graphics.Color.red(pixel)
            val green = android.graphics.Color.green(pixel)
            val blue = android.graphics.Color.blue(pixel)
            val brightest = maxOf(red, green, blue)
            val saturation = brightest - minOf(red, green, blue)
            if (android.graphics.Color.alpha(pixel) >= 220 && brightest >= 32 && saturation >= 20) {
                val weight = (saturation + 16).toLong()
                val group = channels[y / 8]
                group[0] += red * weight
                group[1] += green * weight
                group[2] += blue * weight
                group[3] += weight
            }
        }
    }
    fun tone(index: Int, default: Color): Color {
        val group = channels[index]
        if (group[3] == 0L) return default
        return Color(
            (group[0] / group[3]).toInt(),
            (group[1] / group[3]).toInt(),
            (group[2] / group[3]).toInt(),
        )
    }
    return ArtworkTones(tone(0, fallback.primary), tone(1, fallback.secondary))
}

@Composable
fun AmbientBackdrop(
    tones: ArtworkTones,
    modifier: Modifier = Modifier,
    strength: Float = 0.32f,
    base: Color = MaterialTheme.colorScheme.surface,
    animated: Boolean = false,
    content: @Composable BoxScope.() -> Unit,
) {
    val primary =
        animateColorAsState(
            tones.primary,
            tween(850, easing = FastOutSlowInEasing),
            label = "Artwork primary tone",
        )
    val secondary =
        animateColorAsState(
            tones.secondary,
            tween(850, easing = FastOutSlowInEasing),
            label = "Artwork secondary tone",
        )
    val surface = base
    val opacity = strength * if (surface.luminance() > 0.5f) 0.65f else 0.95f
    val transition = rememberInfiniteTransition(label = "Ambient fluid motion")
    val phase by
        if (animated) {
            transition.animateFloat(
                initialValue = 0f,
                targetValue = 6.2831855f,
                animationSpec =
                    infiniteRepeatable(
                        animation = tween(14000, easing = LinearEasing),
                        repeatMode = RepeatMode.Restart,
                    ),
                label = "Ambient phase",
            )
        } else {
            remember { mutableFloatStateOf(0f) }
        }
    Box(
        modifier.drawBehind {
            val p = phase.toDouble()
            val ox1 = (sin(p) * 0.14).toFloat()
            val oy1 = (cos(p) * 0.12).toFloat()
            val ox2 = (cos(p * 0.75) * 0.15).toFloat()
            val oy2 = (sin(p * 0.75) * 0.13).toFloat()
            val ox3 = (sin(p * 1.35) * 0.12).toFloat()
            val oy3 = (cos(p * 1.35) * 0.10).toFloat()
            val first =
                Brush.radialGradient(
                    listOf(primary.value.copy(alpha = opacity), Color.Transparent),
                    center = Offset(size.width * (0.16f + ox1), size.height * (0.18f + oy1)),
                    radius = size.width * 1.25f,
                )
            val second =
                Brush.radialGradient(
                    listOf(secondary.value.copy(alpha = opacity * 0.85f), Color.Transparent),
                    center = Offset(size.width * (0.86f + ox2), size.height * (0.44f + oy2)),
                    radius = size.width * 1.15f,
                )
            val third =
                Brush.radialGradient(
                    listOf(primary.value.copy(alpha = opacity * 0.55f), Color.Transparent),
                    center = Offset(size.width * (0.50f + ox3), size.height * (0.88f + oy3)),
                    radius = size.width * 0.95f,
                )
            drawRect(surface)
            drawRect(first)
            drawRect(second)
            drawRect(third)
        },
        content = content,
    )
}

@Composable
fun PageReveal(
    destination: Any,
    modifier: Modifier = Modifier,
    content: @Composable ColumnScope.() -> Unit,
) {
    val progress = remember { Animatable(1f) }
    LaunchedEffect(destination) {
        progress.snapTo(0f)
        progress.animateTo(1f, tween(240, easing = FastOutSlowInEasing))
    }
    Column(
        modifier.graphicsLayer {
            alpha = progress.value
            translationY = (1f - progress.value) * 8.dp.toPx()
        },
        content = content,
    )
}

@Composable
fun ArtworkSpotlight(
    modifier: Modifier = Modifier,
    playing: Boolean = true,
    shape: Shape = RoundedCornerShape(28.dp),
    glowColor: Color = MaterialTheme.colorScheme.primary,
    content: @Composable () -> Unit,
) {
    val scale =
        animateFloatAsState(
            if (playing) 1f else 0.92f,
            spring(
                dampingRatio = Spring.DampingRatioMediumBouncy,
                stiffness = Spring.StiffnessLow,
            ),
            label = "Artwork playback scale",
        )
    val glowAlpha =
        animateFloatAsState(
            if (playing) 0.32f else 0.08f,
            tween(500),
            label = "Artwork glow alpha",
        )
    val outline = MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.35f)
    Box(
        modifier
            .graphicsLayer {
                scaleX = scale.value
                scaleY = scale.value
            }
            .drawWithCache {
                val glow =
                    Brush.radialGradient(
                        colors = listOf(glowColor.copy(alpha = glowAlpha.value), Color.Transparent),
                        center = Offset(size.width / 2f, size.height / 2f),
                        radius = size.maxDimension * 0.75f,
                    )
                onDrawBehind { drawRect(glow) }
            }
            .shadow(if (playing) 24.dp else 10.dp, shape)
    ) {
        Surface(shape = shape, border = BorderStroke(1.dp, outline)) {
            Box(Modifier.fillMaxSize()) { content() }
        }
    }
}

@Composable
fun PlayingEqualizerBars(
    playing: Boolean,
    modifier: Modifier = Modifier,
    color: Color = MaterialTheme.colorScheme.primary,
) {
    val transition = rememberInfiniteTransition(label = "Equalizer bars animation")
    val bar1 by
        if (playing) {
            transition.animateFloat(
                initialValue = 0.25f,
                targetValue = 0.95f,
                animationSpec =
                    infiniteRepeatable(
                        animation = tween(420, easing = FastOutSlowInEasing),
                        repeatMode = RepeatMode.Reverse,
                    ),
                label = "Equalizer bar 1",
            )
        } else {
            remember { mutableFloatStateOf(0.35f) }
        }
    val bar2 by
        if (playing) {
            transition.animateFloat(
                initialValue = 0.9f,
                targetValue = 0.2f,
                animationSpec =
                    infiniteRepeatable(
                        animation = tween(540, easing = FastOutSlowInEasing),
                        repeatMode = RepeatMode.Reverse,
                    ),
                label = "Equalizer bar 2",
            )
        } else {
            remember { mutableFloatStateOf(0.7f) }
        }
    val bar3 by
        if (playing) {
            transition.animateFloat(
                initialValue = 0.35f,
                targetValue = 1f,
                animationSpec =
                    infiniteRepeatable(
                        animation = tween(380, easing = FastOutSlowInEasing),
                        repeatMode = RepeatMode.Reverse,
                    ),
                label = "Equalizer bar 3",
            )
        } else {
            remember { mutableFloatStateOf(0.45f) }
        }
    Canvas(modifier.size(16.dp)) {
        val width = size.width
        val height = size.height
        val barWidth = width / 5f
        val cornerRadius = CornerRadius(barWidth / 2f, barWidth / 2f)
        val h1 = (height * bar1).coerceAtLeast(barWidth)
        val h2 = (height * bar2).coerceAtLeast(barWidth)
        val h3 = (height * bar3).coerceAtLeast(barWidth)
        drawRoundRect(
            color = color,
            topLeft = Offset(0f, height - h1),
            size = Size(barWidth, h1),
            cornerRadius = cornerRadius,
        )
        drawRoundRect(
            color = color,
            topLeft = Offset(barWidth * 2f, height - h2),
            size = Size(barWidth, h2),
            cornerRadius = cornerRadius,
        )
        drawRoundRect(
            color = color,
            topLeft = Offset(barWidth * 4f, height - h3),
            size = Size(barWidth, h3),
            cornerRadius = cornerRadius,
        )
    }
}

@Composable
fun AudioFormatBadge(track: Track?, modifier: Modifier = Modifier) {
    if (track == null || track.codec.isBlank()) return
    val rateKhz = (track.rate.toDouble() / 1000).toString().removeSuffix(".0")
    val upperCodec = track.codec.uppercase()
    val isLossless =
        upperCodec.contains("FLAC") ||
            upperCodec.contains("WAV") ||
            upperCodec.contains("ALAC") ||
            upperCodec.contains("PCM")
    Surface(
        shape = RoundedCornerShape(8.dp),
        color =
            if (isLossless) MaterialTheme.colorScheme.primaryContainer.copy(alpha = 0.5f)
            else MaterialTheme.colorScheme.surfaceContainerHighest.copy(alpha = 0.7f),
        border =
            BorderStroke(
                1.dp,
                if (isLossless) MaterialTheme.colorScheme.primary.copy(alpha = 0.25f)
                else MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.3f),
            ),
        modifier = modifier,
    ) {
        Row(
            modifier = Modifier.padding(horizontal = 7.dp, vertical = 2.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(4.dp),
        ) {
            Text(
                upperCodec,
                style = MaterialTheme.typography.labelSmall,
                fontWeight = FontWeight.Bold,
                color =
                    if (isLossless) MaterialTheme.colorScheme.primary
                    else MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Text(
                "$rateKhz kHz",
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}
