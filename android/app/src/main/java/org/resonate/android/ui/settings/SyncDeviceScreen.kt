package org.resonate.android.ui.settings

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.resonate.android.R
import org.resonate.android.ui.LocalResonate
import org.resonate.android.ui.components.Page
import org.resonate.android.words

@Composable
fun SyncDeviceScreen(device: String) {
    val resonate = LocalResonate.current
    val scope = rememberCoroutineScope()
    val status by resonate.features.syncStatus.collectAsStateWithLifecycle()
    val found = status?.devices?.firstOrNull { it.id == device }
    val title = found?.name?.ifBlank { null } ?: stringResource(R.string.sync_unnamed_device)
    Page(title, subtitle = found?.let { deviceState(words(), it) }) { padding ->
        Column(
            Modifier.fillMaxSize()
                .verticalScroll(rememberScrollState())
                .padding(padding)
                .testTag("sync-device")
        ) {
            DrivenDevice(resonate, device) { zone, control, queueRevision ->
                resonate.attempt(scope) { native ->
                    val answer =
                        withContext(Dispatchers.IO) { native.syncControl(device, zone, control, queueRevision) }
                    DrivenWords.refusal(resonate.library.words, answer)?.let { resonate.library.notice(it) }
                }
            }
        }
    }
}
