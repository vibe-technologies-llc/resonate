package org.resonate.android.ui.components

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.unit.Dp

private class Bar(val x: Float, val top: Float, val bottom: Float)

private const val MARK_VIEWPORT = 24f
private const val BAR_WIDTH = 2f

private val MARK_BARS =
    listOf(
        Bar(4f, 10f, 14f),
        Bar(8f, 6f, 18f),
        Bar(12f, 3.5f, 20.5f),
        Bar(16f, 7.5f, 16.5f),
        Bar(20f, 10f, 14f),
    )

@Composable
fun ResonateMark(size: Dp, color: Color, modifier: Modifier = Modifier) {
    Canvas(modifier.size(size)) {
        val unit = this.size.minDimension / MARK_VIEWPORT
        MARK_BARS.forEach { bar ->
            drawLine(
                color = color,
                start = Offset(bar.x * unit, bar.top * unit),
                end = Offset(bar.x * unit, bar.bottom * unit),
                strokeWidth = BAR_WIDTH * unit,
                cap = StrokeCap.Round,
            )
        }
    }
}
