package org.resonate.android

import android.graphics.Bitmap
import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.FastOutSlowInEasing
import androidx.compose.animation.core.Spring
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.spring
import androidx.compose.animation.core.tween
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.State
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawWithCache
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.unit.dp
import androidx.core.graphics.get
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
    content: @Composable BoxScope.() -> Unit,
) {
    val primary = animateColorAsState(tones.primary, tween(850), label = "Artwork primary tone")
    val secondary =
        animateColorAsState(tones.secondary, tween(850), label = "Artwork secondary tone")
    val surface = base
    val opacity = strength * if (surface.luminance() > 0.5f) 0.65f else 1f
    Box(
        modifier.drawWithCache {
            val first =
                Brush.radialGradient(
                    listOf(primary.value.copy(alpha = opacity), Color.Transparent),
                    center = Offset(size.width * 0.12f, size.height * 0.18f),
                    radius = size.width * 1.1f,
                )
            val second =
                Brush.radialGradient(
                    listOf(secondary.value.copy(alpha = opacity * 0.7f), Color.Transparent),
                    center = Offset(size.width * 0.95f, size.height * 0.5f),
                    radius = size.width * 0.95f,
                )
            onDrawBehind {
                drawRect(surface)
                drawRect(first)
                drawRect(second)
            }
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
    shape: Shape = RoundedCornerShape(24.dp),
    content: @Composable () -> Unit,
) {
    val scale =
        animateFloatAsState(
            if (playing) 1f else 0.93f,
            spring(dampingRatio = Spring.DampingRatioNoBouncy, stiffness = Spring.StiffnessLow),
            label = "Artwork playback scale",
        )
    val outline = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.12f)
    Box(
        modifier
            .graphicsLayer {
                scaleX = scale.value
                scaleY = scale.value
            }
            .shadow(24.dp, shape)
    ) {
        androidx.compose.material3.Surface(shape = shape, border = BorderStroke(1.dp, outline)) {
            Box(Modifier.fillMaxSize()) { content() }
        }
    }
}
