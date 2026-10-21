package org.resonate.android.ui.components

import androidx.compose.foundation.background
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Slider
import androidx.compose.material3.SliderDefaults
import androidx.compose.material3.SliderState
import androidx.compose.ui.graphics.Color
import androidx.compose.runtime.Composable
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
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
) {
    var dragging by remember { mutableStateOf(false) }
    val state = remember(range) { SliderState(value.coerceIn(range), 0, range) }
    SideEffect { if (!dragging) state.value = value.coerceIn(range) }
    val track: @Composable (SliderState) -> Unit =
        if (trackHeight == null) ({ SliderDefaults.Track(it, enabled = enabled) })
        else ({ slider -> ThinTrack(slider, range, trackHeight, enabled) })
    val interaction = remember { MutableInteractionSource() }
    val thumb: @Composable (SliderState) -> Unit =
        if (trackHeight == null) ({ SliderDefaults.Thumb(interaction, enabled = enabled) })
        else ({ SliderDefaults.Thumb(interaction, enabled = enabled, thumbSize = DpSize(THUMB_WIDTH, THUMB_HEIGHT)) })
    Slider(
        state = state,
        enabled = enabled,
        modifier = modifier,
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
) {
    val scheme = MaterialTheme.colorScheme
    val span = range.endInclusive - range.start
    val fraction = if (span > 0f) ((slider.value - range.start) / span).coerceIn(0f, 1f) else 0f
    val active = if (enabled) scheme.primary else scheme.onSurface.copy(alpha = 0.38f)
    val inactive: Color = if (enabled) scheme.secondaryContainer else scheme.onSurface.copy(alpha = 0.12f)
    Box(Modifier.fillMaxWidth().height(height).clip(CircleShape).background(inactive)) {
        Box(Modifier.fillMaxWidth(fraction).fillMaxHeight().background(active))
    }
}

private val THUMB_WIDTH = 4.dp
private val THUMB_HEIGHT = 20.dp
