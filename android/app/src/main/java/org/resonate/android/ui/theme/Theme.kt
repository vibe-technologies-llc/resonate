package org.resonate.android.ui.theme

import android.graphics.Bitmap
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialExpressiveTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.MotionScheme
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.produceState
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.platform.LocalContext
import com.materialkolor.DynamicMaterialExpressiveTheme
import com.materialkolor.PaletteStyle
import com.materialkolor.ktx.themeColorOrNull
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

val ResonateViolet = Color(0xFF8772E7)

@Composable
fun ResonateTheme(content: @Composable () -> Unit) {
    val context = LocalContext.current
    val dark = isSystemInDarkTheme()
    MaterialExpressiveTheme(
        colorScheme = if (dark) dynamicDarkColorScheme(context) else dynamicLightColorScheme(context),
        motionScheme = MotionScheme.expressive(),
        content = content,
    )
}

@Composable
fun ArtworkTheme(seed: Color?, content: @Composable () -> Unit) {
    DynamicMaterialExpressiveTheme(
        seedColor = seed ?: MaterialTheme.colorScheme.primary,
        motionScheme = MotionScheme.expressive(),
        isDark = isSystemInDarkTheme(),
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
