package org.resonate.android.ui.together

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.ContentCopy
import androidx.compose.material.icons.rounded.QrCode2
import androidx.compose.material.icons.rounded.Share
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.setValue
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalClipboard
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import org.resonate.android.R
import org.resonate.android.ui.LocalResonate
import org.resonate.android.ui.components.Sizes
import org.resonate.android.ui.settings.ButtonRow
import org.resonate.android.ui.settings.rememberCopy

private val QR_SIDE = 200.dp
private const val SECOND_PRESS_WAITS_MS = 4_000L

data class Pressing<K>(val armed: K?, val confirmed: Boolean)

fun <K> pressed(armed: K?, key: K): Pressing<K> =
    if (armed == key) Pressing(null, confirmed = true) else Pressing(key, confirmed = false)

@Composable
fun rememberSecondPress(): SecondPress {
    val press = rememberSaveable(saver = SecondPress.saver) { SecondPress() }
    LaunchedEffect(press.armed) {
        if (press.armed != null) {
            delay(SECOND_PRESS_WAITS_MS)
            press.armed = null
        }
    }
    return press
}

class SecondPress {
    var armed by mutableStateOf<String?>(null)

    fun press(key: String, confirmed: () -> Unit) {
        val pressing = pressed(armed, key)
        armed = pressing.armed
        if (pressing.confirmed) confirmed()
    }

    @Composable
    fun label(key: String, label: String): String =
        if (armed == key) stringResource(R.string.together_press_again) else label

    companion object {
        val saver =
            androidx.compose.runtime.saveable.Saver<SecondPress, String>(
                save = { it.armed.orEmpty() },
                restore = { saved -> SecondPress().apply { armed = saved.ifEmpty { null } } },
            )
    }
}

@Composable
fun InvitationLink(
    saying: String,
    link: String,
    shareTitle: String,
    tag: String,
    modifier: Modifier = Modifier,
    qrAtFirst: Boolean = false,
) {
    val resonate = LocalResonate.current
    val copy = rememberCopy()
    var qrShown by rememberSaveable(link) { mutableStateOf(qrAtFirst) }
    Column(modifier.fillMaxWidth()) {
        Text(
            saying,
            style = MaterialTheme.typography.titleSmall,
            modifier = Modifier.padding(horizontal = Sizes.gutter, vertical = 4.dp),
        )
        SelectionContainer {
            Text(
                link,
                style =
                    MaterialTheme.typography.bodySmall.copy(fontFamily = FontFamily.Monospace),
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier =
                    Modifier.padding(horizontal = Sizes.gutter, vertical = 4.dp)
                        .testTag("$tag-link"),
            )
        }
        if (qrShown) {
            Box(
                Modifier.fillMaxWidth().padding(vertical = 8.dp),
                contentAlignment = Alignment.Center,
            ) {
                QrCode(link, QR_SIDE, Modifier.testTag("$tag-qr"))
            }
        }
        ButtonRow(start = true) {
            OutlinedButton(onClick = { copy(saying, link) }, modifier = Modifier.testTag("$tag-copy")) {
                Icon(Icons.Rounded.ContentCopy, null, Modifier.size(18.dp))
                Spacer(Modifier.width(8.dp))
                Text(stringResource(R.string.together_copy))
            }
            FilledTonalButton(
                onClick = { resonate.host.share(shareTitle, link) },
                modifier = Modifier.testTag("$tag-share"),
            ) {
                Icon(Icons.Rounded.Share, null, Modifier.size(18.dp))
                Spacer(Modifier.width(8.dp))
                Text(stringResource(R.string.together_share))
            }
            TextButton(onClick = { qrShown = !qrShown }, modifier = Modifier.testTag("$tag-toggle-qr")) {
                Icon(Icons.Rounded.QrCode2, null, Modifier.size(18.dp))
                Spacer(Modifier.width(8.dp))
                Text(
                    stringResource(
                        if (qrShown) R.string.together_hide_qr_code else R.string.together_show_qr_code
                    )
                )
            }
        }
    }
}

@Composable
fun rememberPasteALink(): () -> Unit {
    val resonate = LocalResonate.current
    val clipboard = LocalClipboard.current
    val scope = rememberCoroutineScope()
    return {
        scope.launch {
            val text = clipboard.getClipEntry()?.clipData?.takeIf { it.itemCount > 0 }?.getItemAt(0)?.text
            resonate.features.together.paste(text?.toString())
        }
    }
}
