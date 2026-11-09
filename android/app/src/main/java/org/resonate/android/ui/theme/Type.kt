package org.resonate.android.ui.theme

import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Shapes
import androidx.compose.material3.Typography
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

private const val TABULAR_FIGURES = "tnum"

private val ROW_TITLE_TRACKING = 0.15.sp

private val HEADING_TRACKING = (-0.2).sp

private fun TextStyle.figured(): TextStyle = copy(fontFeatureSettings = TABULAR_FIGURES)

private fun TextStyle.heading(): TextStyle = figured().copy(letterSpacing = HEADING_TRACKING)

val ResonateTypography: Typography =
    Typography().run {
        copy(
            displayLarge = displayLarge.heading(),
            displayMedium = displayMedium.heading(),
            displaySmall = displaySmall.heading(),
            headlineLarge = headlineLarge.heading(),
            headlineMedium = headlineMedium.heading(),
            headlineSmall = headlineSmall.heading(),
            titleLarge = titleLarge.figured(),
            titleMedium = titleMedium.figured(),
            titleSmall = titleSmall.figured(),
            bodyLarge = bodyLarge.figured().copy(letterSpacing = ROW_TITLE_TRACKING),
            bodyMedium = bodyMedium.figured(),
            bodySmall = bodySmall.figured(),
            labelLarge = labelLarge.figured(),
            labelMedium = labelMedium.figured(),
            labelSmall = labelSmall.figured(),
        )
    }

private val FIELD_CORNER = 12.dp

val ResonateShapes: Shapes = Shapes().copy(extraSmall = RoundedCornerShape(FIELD_CORNER))
