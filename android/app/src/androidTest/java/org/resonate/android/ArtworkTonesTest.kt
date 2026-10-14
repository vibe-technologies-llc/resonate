package org.resonate.android

import android.graphics.Bitmap
import androidx.compose.ui.graphics.Color
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class ArtworkTonesTest {
    @Test
    fun artworkTonesPreserveCoverColorsAndFallBackForNeutralOrTransparentArt() {
        val fallback = ArtworkTones(Color.Cyan, Color.Magenta)
        val cover = Bitmap.createBitmap(32, 32, Bitmap.Config.ARGB_8888)
        try {
            repeat(32) { y ->
                repeat(32) { x ->
                    cover.setPixel(
                        x,
                        y,
                        if (y < 16) android.graphics.Color.RED else android.graphics.Color.BLUE,
                    )
                }
            }
            assertEquals(ArtworkTones(Color.Red, Color.Blue), artworkTones(cover, fallback))
            cover.eraseColor(android.graphics.Color.GRAY)
            assertEquals(fallback, artworkTones(cover, fallback))
            cover.eraseColor(android.graphics.Color.TRANSPARENT)
            assertEquals(fallback, artworkTones(cover, fallback))
        } finally {
            cover.recycle()
        }
        val tiny = Bitmap.createBitmap(1, 1, Bitmap.Config.ARGB_8888)
        try {
            tiny.eraseColor(android.graphics.Color.GREEN)
            assertEquals(ArtworkTones(Color.Green, Color.Green), artworkTones(tiny, fallback))
        } finally {
            tiny.recycle()
        }
    }
}
