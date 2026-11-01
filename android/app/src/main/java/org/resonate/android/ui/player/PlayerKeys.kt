package org.resonate.android.ui.player

import androidx.compose.ui.Modifier
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.semantics.CustomAccessibilityAction

const val SKIP_MS = 10_000L

fun PlayerViewModel.skipBy(deltaMs: Long, durationMs: Long) {
    if (durationMs <= 0) return
    seek((position.value + deltaMs).coerceIn(0L, durationMs))
}

fun PlayerViewModel.skipActions(durationMs: Long): List<CustomAccessibilityAction> =
    listOf(
        CustomAccessibilityAction("Back 10 seconds") {
            skipBy(-SKIP_MS, durationMs)
            true
        },
        CustomAccessibilityAction("Forward 10 seconds") {
            skipBy(SKIP_MS, durationMs)
            true
        },
    )

fun Modifier.playerKeys(model: PlayerViewModel, durationMs: () -> Long): Modifier = onKeyEvent {
    if (it.type != KeyEventType.KeyDown) return@onKeyEvent false
    when (it.key) {
        Key.Spacebar,
        Key.MediaPlayPause -> {
            model.toggle()
            true
        }
        Key.DirectionLeft,
        Key.MediaRewind -> {
            model.skipBy(-SKIP_MS, durationMs())
            true
        }
        Key.DirectionRight,
        Key.MediaFastForward -> {
            model.skipBy(SKIP_MS, durationMs())
            true
        }
        else -> false
    }
}
