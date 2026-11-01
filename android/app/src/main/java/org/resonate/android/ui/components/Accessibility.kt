package org.resonate.android.ui.components

import android.content.Context
import android.provider.Settings
import android.view.accessibility.AccessibilityManager
import androidx.compose.material3.ColorScheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.SliderColors
import androidx.compose.material3.SliderDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.compositeOver
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner

data class Calm(val reduced: Boolean, val spoken: Boolean)

private fun calmOf(context: Context): Calm {
    val scale =
        Settings.Global.getFloat(
            context.contentResolver,
            Settings.Global.ANIMATOR_DURATION_SCALE,
            1f,
        )
    val access = context.getSystemService(AccessibilityManager::class.java)
    return Calm(reduced = scale == 0f, spoken = access?.isTouchExplorationEnabled == true)
}

@Composable
fun rememberCalm(): Calm {
    val context = LocalContext.current
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    var calm by remember(context) { mutableStateOf(calmOf(context)) }
    DisposableEffect(context, lifecycle) {
        val access = context.getSystemService(AccessibilityManager::class.java)
        val exploring =
            AccessibilityManager.TouchExplorationStateChangeListener { calm = calmOf(context) }
        val resumed = LifecycleEventObserver { _, event ->
            if (event == Lifecycle.Event.ON_RESUME) calm = calmOf(context)
        }
        access?.addTouchExplorationStateChangeListener(exploring)
        lifecycle.addObserver(resumed)
        onDispose {
            access?.removeTouchExplorationStateChangeListener(exploring)
            lifecycle.removeObserver(resumed)
        }
    }
    return calm
}

const val LARGE_TEXT_SCALE = 1.3f
const val LARGEST_TEXT_SCALE = 1.8f

fun titleLinesAt(fontScale: Float): Int =
    when {
        fontScale > LARGEST_TEXT_SCALE -> 3
        fontScale > LARGE_TEXT_SCALE -> 2
        else -> 1
    }

@Composable
fun rememberTitleMaxLines(): Int = titleLinesAt(LocalDensity.current.fontScale)

@Composable
fun rememberLargeText(): Boolean = LocalDensity.current.fontScale > LARGE_TEXT_SCALE

fun recommendedTimeoutMs(context: Context, original: Long, withAction: Boolean = false): Long {
    val access = context.getSystemService(AccessibilityManager::class.java) ?: return original
    val flags =
        AccessibilityManager.FLAG_CONTENT_TEXT or
            if (withAction) AccessibilityManager.FLAG_CONTENT_CONTROLS else 0
    return access
        .getRecommendedTimeoutMillis(original.coerceAtMost(Int.MAX_VALUE.toLong()).toInt(), flags)
        .toLong()
}

@Composable
fun outlinedTrackColors(): SliderColors =
    SliderDefaults.colors(inactiveTrackColor = MaterialTheme.colorScheme.outline)

const val BANNER_TRACK_ALPHA = 0.7f

fun bannerTrackOf(scheme: ColorScheme): Color =
    scheme.onSecondaryContainer.copy(alpha = BANNER_TRACK_ALPHA).compositeOver(scheme.secondaryContainer)
