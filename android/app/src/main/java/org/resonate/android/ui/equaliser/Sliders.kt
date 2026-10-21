package org.resonate.android.ui.equaliser

import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.material3.Slider
import androidx.compose.material3.SliderDefaults
import androidx.compose.material3.SliderState
import androidx.compose.material3.VerticalSlider
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.unit.DpSize
import androidx.compose.ui.unit.dp

enum class SliderLook {
    PLAIN,
    CENTRED,
}

@Composable
fun CommitSlider(
    value: Float,
    range: ClosedFloatingPointRange<Float>,
    description: String,
    stateText: (Float) -> String,
    onCommit: (Float) -> Unit,
    modifier: Modifier = Modifier,
    vertical: Boolean = false,
    look: SliderLook = SliderLook.PLAIN,
    snap: (Float) -> Float = { it },
    onDraft: (Float?) -> Unit = {},
) {
    var draft by remember { mutableStateOf<Float?>(null) }
    val state = remember(range) { SliderState(value.coerceIn(range), 0, range) }
    LaunchedEffect(value, range, draft == null) {
        if (draft == null) state.value = value.coerceIn(range)
    }
    val change: (Float) -> Unit = { moved ->
        val snapped = snap(moved).coerceIn(range)
        state.value = snapped
        draft = snapped
        onDraft(snapped)
    }
    val finish: () -> Unit = {
        draft?.let(onCommit)
        draft = null
        onDraft(null)
    }
    val shown = draft ?: value
    val interactions = remember { MutableInteractionSource() }
    val colors = SliderDefaults.colors()
    val labelled =
        modifier.semantics {
            contentDescription = description
            stateDescription = stateText(shown)
        }
    val track: @Composable (SliderState) -> Unit = { sliderState ->
        when (look) {
            SliderLook.CENTRED ->
                SliderDefaults.CenteredTrack(
                    sliderState = sliderState,
                    colors = colors,
                    drawStopIndicator = null,
                )
            SliderLook.PLAIN ->
                SliderDefaults.Track(
                    sliderState = sliderState,
                    colors = colors,
                    drawStopIndicator = null,
                )
        }
    }
    if (vertical) {
        VerticalSlider(
            state = state,
            onValueChange = change,
            onValueChangeFinished = finish,
            topToBottom = false,
            interactionSource = interactions,
            colors = colors,
            modifier = labelled,
            thumb = {
                SliderDefaults.Thumb(
                    interactionSource = interactions,
                    colors = colors,
                    thumbSize = VERTICAL_THUMB,
                    isVertical = true,
                )
            },
            track = track,
        )
    } else {
        Slider(
            state = state,
            onValueChange = change,
            onValueChangeFinished = finish,
            interactionSource = interactions,
            colors = colors,
            modifier = labelled.fillMaxWidth(),
            track = track,
        )
    }
}

@Composable
fun LogCommitSlider(
    value: Float,
    range: ClosedFloatingPointRange<Float>,
    description: String,
    format: (Float) -> String,
    onCommit: (Float) -> Unit,
    modifier: Modifier = Modifier,
    onDraft: (Float?) -> Unit = {},
) {
    val scale = remember(range) { LogScale(range.start, range.endInclusive) }
    CommitSlider(
        value = scale.fraction(value),
        range = 0f..1f,
        description = description,
        stateText = { format(scale.actual(it)) },
        onCommit = { onCommit(scale.actual(it)) },
        onDraft = { fraction -> onDraft(fraction?.let(scale::actual)) },
        modifier = modifier,
    )
}

private val VERTICAL_THUMB = DpSize(28.dp, 4.dp)
