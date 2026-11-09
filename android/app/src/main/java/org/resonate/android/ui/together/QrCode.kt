package org.resonate.android.ui.together

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.size
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.Dp
import com.google.zxing.BarcodeFormat
import com.google.zxing.EncodeHintType
import com.google.zxing.WriterException
import com.google.zxing.qrcode.QRCodeWriter
import com.google.zxing.qrcode.decoder.ErrorCorrectionLevel
import org.resonate.android.R

const val QUIET_MODULES = 4

class QrModules(val side: Int, private val dark: BooleanArray) {
    fun isDark(column: Int, row: Int): Boolean = dark[row * side + column]

    companion object {
        fun of(text: String): QrModules? {
            val drawn =
                try {
                    QRCodeWriter()
                        .encode(
                            text,
                            BarcodeFormat.QR_CODE,
                            0,
                            0,
                            mapOf(
                                EncodeHintType.MARGIN to 0,
                                EncodeHintType.ERROR_CORRECTION to ErrorCorrectionLevel.M,
                            ),
                        )
                } catch (refused: WriterException) {
                    return null
                }
            val side = drawn.width
            return QrModules(
                side,
                BooleanArray(side * side) { at -> drawn.get(at % side, at / side) },
            )
        }
    }
}

@Composable
fun QrCode(text: String, side: Dp, modifier: Modifier = Modifier) {
    val modules = remember(text) { QrModules.of(text) } ?: return
    val described = stringResource(R.string.together_qr_code)
    Canvas(
        modifier.size(side).clip(MaterialTheme.shapes.medium).semantics {
            contentDescription = described
        }
    ) {
        drawRect(Color.White)
        val across = modules.side + 2 * QUIET_MODULES
        val module = size.width / across
        for (row in 0 until modules.side) {
            for (column in 0 until modules.side) {
                if (modules.isDark(column, row)) {
                    drawRect(
                        Color.Black,
                        topLeft =
                            Offset((column + QUIET_MODULES) * module, (row + QUIET_MODULES) * module),
                        size = Size(module, module),
                    )
                }
            }
        }
    }
}
