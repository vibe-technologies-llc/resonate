package org.resonate.android.ui.components

import androidx.compose.material3.Slider
import androidx.compose.material3.SliderState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier

@Composable
fun CommitSlider(
    value: Float,
    onCommit: (Float) -> Unit,
    modifier: Modifier = Modifier,
    range: ClosedFloatingPointRange<Float> = 0f..1f,
    enabled: Boolean = true,
    onDraft: (Float?) -> Unit = {},
) {
    var dragging by remember { mutableStateOf(false) }
    val state = remember(range) { SliderState(value.coerceIn(range), 0, range) }
    SideEffect { if (!dragging) state.value = value.coerceIn(range) }
    Slider(
        state = state,
        enabled = enabled,
        modifier = modifier,
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
