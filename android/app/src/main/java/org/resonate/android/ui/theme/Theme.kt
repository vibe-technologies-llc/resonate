package org.resonate.android.ui.theme

import android.content.Context
import android.graphics.Bitmap
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
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.platform.LocalContext
import com.materialkolor.DynamicMaterialExpressiveTheme
import com.materialkolor.PaletteStyle
import com.materialkolor.dynamicColorScheme
import com.materialkolor.ktx.themeColorOrNull
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.resonate.android.Appearance
import org.resonate.android.ThemeMode

val ResonateViolet = Color(0xFF8772E7)

val LocalDarkTheme = staticCompositionLocalOf { false }

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
    val scheme =
        remember(accent, dark, context) {
            val seed = accent?.let { Color(0xFF000000.toInt() or it.hex) } ?: wallpaperSeed(context)
            schemeOf(seed, dark)
        }
    SystemBarsFollow(dark)
    CompositionLocalProvider(LocalDarkTheme provides dark) {
        MaterialExpressiveTheme(
            colorScheme = scheme,
            motionScheme = MotionScheme.expressive(),
            content = content,
        )
    }
}

fun wallpaperSeed(context: Context): Color =
    Color(context.getColor(android.R.color.system_accent1_500))

fun schemeOf(seed: Color, dark: Boolean): ColorScheme =
    dynamicColorScheme(
        seedColor = seed,
        isDark = dark,
        isAmoled = false,
        style = PaletteStyle.TonalSpot,
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
        style = PaletteStyle.TonalSpot,
        animate = true,
        content = content,
    )
}

@Composable
fun rememberSeedOf(bitmap: Bitmap?): Color? {
    val seed by
        produceState<Color?>(null, bitmap) {
            value = bitmap?.let { withContext(Dispatchers.Default) { seedOf(it.asImageBitmap()) } }
        }
    return seed
}

private fun seedOf(image: ImageBitmap): Color? = image.themeColorOrNull(filter = true)
