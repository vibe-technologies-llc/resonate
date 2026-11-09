package org.resonate.android.ui.components

import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Slider
import androidx.compose.material3.SliderDefaults
import androidx.compose.material3.SliderState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.compose.ui.semantics.customActions
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.DpSize
import androidx.compose.ui.unit.dp

@Composable
fun CommitSlider(
    value: Float,
    onCommit: (Float) -> Unit,
    modifier: Modifier = Modifier,
    range: ClosedFloatingPointRange<Float> = 0f..1f,
    enabled: Boolean = true,
    onDraft: (Float?) -> Unit = {},
    trackHeight: Dp? = null,
    stateText: ((Float) -> String)? = null,
    skips: List<CustomAccessibilityAction> = emptyList(),
    centred: Boolean = false,
) {
    var dragging by remember { mutableStateOf(false) }
    val state = remember(range) { SliderState(value.coerceIn(range), 0, range) }
    SideEffect { if (!dragging) state.value = value.coerceIn(range) }
    val outlinedColors = outlinedTrackColors()
    val track: @Composable (SliderState) -> Unit =
        if (trackHeight == null)
            ({
                SliderDefaults.Track(
                    it,
                    enabled = enabled,
                    colors = outlinedColors,
                )
            })
        else ({ slider -> ThinTrack(slider, range, trackHeight, enabled, centred) })
    val interaction = remember { MutableInteractionSource() }
    val thumb: @Composable (SliderState) -> Unit =
        if (trackHeight == null) ({ SliderDefaults.Thumb(interaction, enabled = enabled) })
        else ({ SliderDefaults.Thumb(interaction, enabled = enabled, thumbSize = DpSize(THUMB_WIDTH, THUMB_HEIGHT)) })
    Slider(
        state = state,
        enabled = enabled,
        modifier =
            modifier.semantics {
                if (stateText != null) stateDescription = stateText(state.value)
                if (skips.isNotEmpty()) customActions = skips
            },
        interactionSource = interaction,
        thumb = thumb,
        track = track,
        onValueChange = { moved ->
            dragging = true
            state.value = moved
            onDraft(moved)
        },
        onValueChangeFinished = {
            onCommit(state.value)
            dragging = false
            onDraft(null)
        },
    )
}

@Composable
private fun ThinTrack(
    slider: SliderState,
    range: ClosedFloatingPointRange<Float>,
    height: Dp,
    enabled: Boolean,
    centred: Boolean,
) {
    val scheme = MaterialTheme.colorScheme
    val fraction = fractionOf(slider.value, range)
    val anchor = if (centred) fractionOf(0f, range) else 0f
    val active = if (enabled) scheme.primary else scheme.onSurface.copy(alpha = 0.38f)
    val inactive: Color = if (enabled) scheme.outline else scheme.onSurface.copy(alpha = 0.12f)
    Box(
        Modifier.fillMaxWidth().height(height).clip(CircleShape).drawBehind {
            val from = minOf(anchor, fraction) * size.width
            val to = maxOf(anchor, fraction) * size.width
            drawRect(inactive)
            drawRect(active, topLeft = Offset(from, 0f), size = Size(to - from, size.height))
        }
    )
}

private fun fractionOf(value: Float, range: ClosedFloatingPointRange<Float>): Float {
    val span = range.endInclusive - range.start
    return if (span > 0f) ((value - range.start) / span).coerceIn(0f, 1f) else 0f
}

private val THUMB_WIDTH = 4.dp
private val THUMB_HEIGHT = 20.dp
