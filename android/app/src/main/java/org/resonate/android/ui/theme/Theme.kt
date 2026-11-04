package org.resonate.android.ui.theme

import android.app.UiModeManager
import android.content.Context
import android.graphics.Bitmap
import android.os.Build
import android.util.LruCache
import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.compose.LocalActivity
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialExpressiveTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ColorScheme
import androidx.compose.material3.MotionScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableDoubleStateOf
import androidx.compose.runtime.setValue
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.platform.LocalContext
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import com.materialkolor.DynamicMaterialExpressiveTheme
import com.materialkolor.PaletteStyle
import com.materialkolor.dynamicColorScheme
import com.materialkolor.ktx.themeColorOrNull
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext
import org.resonate.android.Appearance
import org.resonate.android.ThemeMode

val ResonateViolet = Color(0xFF8772E7)

val LocalDarkTheme = staticCompositionLocalOf { false }

val LocalPitchBlack = staticCompositionLocalOf { false }

val LocalContrastLevel = staticCompositionLocalOf { STANDARD_CONTRAST }

const val STANDARD_CONTRAST = 0.0
const val HIGH_CONTRAST = 1.0

fun systemContrastLevel(context: Context): Double {
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.UPSIDE_DOWN_CAKE) return STANDARD_CONTRAST
    val modes = context.getSystemService(UiModeManager::class.java) ?: return STANDARD_CONTRAST
    return modes.contrast.toDouble().coerceIn(STANDARD_CONTRAST, HIGH_CONTRAST)
}

@Composable
fun ResonateTheme(appearance: Appearance, content: @Composable () -> Unit) {
    val context = LocalContext.current
    val dark =
        when (appearance.mode) {
            ThemeMode.SYSTEM -> isSystemInDarkTheme()
            ThemeMode.LIGHT -> false
            ThemeMode.DARK -> true
        }
    val accent = appearance.accent
    val pitchBlack = dark && appearance.pitchBlack
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    var system by remember(context) { mutableDoubleStateOf(systemContrastLevel(context)) }
    DisposableEffect(context, lifecycle) {
        val resumed = LifecycleEventObserver { _, event ->
            if (event == Lifecycle.Event.ON_RESUME) system = systemContrastLevel(context)
        }
        lifecycle.addObserver(resumed)
        onDispose { lifecycle.removeObserver(resumed) }
    }
    val contrast = if (appearance.highContrast) HIGH_CONTRAST else system
    val scheme =
        remember(accent, dark, pitchBlack, contrast, context) {
            val seed = accent?.let { Color(0xFF000000.toInt() or it.hex) } ?: wallpaperSeed(context)
            schemeOf(seed, dark, pitchBlack, contrast)
        }
    SystemBarsFollow(dark)
    CompositionLocalProvider(
        LocalDarkTheme provides dark,
        LocalPitchBlack provides pitchBlack,
        LocalContrastLevel provides contrast,
    ) {
        MaterialExpressiveTheme(
            colorScheme = scheme,
            motionScheme = MotionScheme.expressive(),
            content = content,
        )
    }
}

fun wallpaperSeed(context: Context): Color =
    Color(context.getColor(android.R.color.system_accent1_500))

fun schemeOf(
    seed: Color,
    dark: Boolean,
    pitchBlack: Boolean = false,
    contrastLevel: Double = STANDARD_CONTRAST,
): ColorScheme =
    dynamicColorScheme(
        seedColor = seed,
        isDark = dark,
        isAmoled = pitchBlack,
        style = PaletteStyle.TonalSpot,
        contrastLevel = contrastLevel,
    )

@Composable
private fun SystemBarsFollow(dark: Boolean) {
    val activity = LocalActivity.current as? ComponentActivity ?: return
    DisposableEffect(activity, dark) {
        val bars =
            SystemBarStyle.auto(
                lightScrim = android.graphics.Color.TRANSPARENT,
                darkScrim = android.graphics.Color.TRANSPARENT,
                detectDarkMode = { dark },
            )
        activity.enableEdgeToEdge(statusBarStyle = bars, navigationBarStyle = bars)
        onDispose {}
    }
}

@Composable
fun ArtworkTheme(seed: Color?, content: @Composable () -> Unit) {
    DynamicMaterialExpressiveTheme(
        seedColor = seed ?: MaterialTheme.colorScheme.primary,
        motionScheme = MotionScheme.expressive(),
        isDark = LocalDarkTheme.current,
        isAmoled = LocalPitchBlack.current,
        style = PaletteStyle.TonalSpot,
        contrastLevel = LocalContrastLevel.current,
        animate = true,
        content = content,
    )
}

@Composable
fun ArtworkTheme(scheme: ColorScheme, content: @Composable () -> Unit) {
    MaterialExpressiveTheme(
        colorScheme = scheme,
        motionScheme = MotionScheme.expressive(),
        content = content,
    )
}

const val SEED_OUTLASTS_ITS_COVER_MS = 600L

private const val SEEDS_KEPT = 64

private const val SCHEMES_KEPT = 16

private class Seed(val color: Color?)

private val seeds = LruCache<Bitmap, Seed>(SEEDS_KEPT)

private suspend fun seedFor(bitmap: Bitmap): Color? =
    (seeds[bitmap]
            ?: withContext(Dispatchers.Default) {
                Seed(seedOf(bitmap.asImageBitmap())).also { seeds.put(bitmap, it) }
            })
        .color

@Composable
fun rememberSeedOf(bitmap: Bitmap?): Color? {
    val known = remember(bitmap) { bitmap?.let { seeds[it] } }
    val seed by
        produceState(known?.color, bitmap) {
            if (bitmap == null) {
                delay(SEED_OUTLASTS_ITS_COVER_MS)
                value = null
            } else {
                value = seedFor(bitmap)
            }
        }
    return if (known != null) known.color else seed
}

private fun seedOf(image: ImageBitmap): Color? = image.themeColorOrNull(filter = true)

private data class SchemeAsked(
    val seed: Color,
    val dark: Boolean,
    val pitchBlack: Boolean,
    val contrast: Double,
) {
    fun made(): ColorScheme =
        schemes[this] ?: schemeOf(seed, dark, pitchBlack, contrast).also { schemes.put(this, it) }
}

private val schemes = LruCache<SchemeAsked, ColorScheme>(SCHEMES_KEPT)

@Composable
fun rememberSchemeOf(seed: Color?): ColorScheme {
    val asked =
        SchemeAsked(
            seed ?: MaterialTheme.colorScheme.primary,
            LocalDarkTheme.current,
            LocalPitchBlack.current,
            LocalContrastLevel.current,
        )
    val known = remember(asked) { schemes[asked] }
    val first = remember { asked.made() }
    val scheme by
        produceState(first, asked) { value = withContext(Dispatchers.Default) { asked.made() } }
    return known ?: scheme
}
