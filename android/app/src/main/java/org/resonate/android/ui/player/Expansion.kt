package org.resonate.android.ui.player

import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.spring
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Stable
import androidx.compose.runtime.saveable.Saver
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.staticCompositionLocalOf
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.launch

@Stable
class PlayerExpansion(private val scope: CoroutineScope, opened: Boolean = false) {
    val progress = Animatable(if (opened) 1f else 0f)

    val shown: Boolean
        get() = progress.value > 0f || progress.isRunning

    val expanded: Boolean
        get() = progress.targetValue == 1f

    fun expand() {
        scope.launch { progress.animateTo(1f, spring(dampingRatio = 0.86f, stiffness = 380f)) }
    }

    fun collapse() {
        scope.launch { progress.animateTo(0f, spring(dampingRatio = 1f, stiffness = 420f)) }
    }

    fun collapseNow() {
        scope.launch { progress.snapTo(0f) }
    }

    suspend fun follow(value: Float) {
        progress.snapTo(value.coerceIn(0f, 1f))
    }

    fun settle(velocity: Float, travel: Float) {
        val towardsOpen = velocity < -FLING_VELOCITY || (velocity <= FLING_VELOCITY && progress.value > 0.6f)
        if (travel > 0f && towardsOpen) expand() else collapse()
    }

    private companion object {
        const val FLING_VELOCITY = 900f
    }
}

@Composable
fun rememberPlayerExpansion(): PlayerExpansion {
    val scope = rememberCoroutineScope()
    return rememberSaveable(
        scope,
        saver = Saver(save = { it.expanded }, restore = { PlayerExpansion(scope, it) }),
    ) {
        PlayerExpansion(scope)
    }
}

val LocalPlayerExpansion =
    staticCompositionLocalOf<PlayerExpansion> { error("No player expansion in composition") }
