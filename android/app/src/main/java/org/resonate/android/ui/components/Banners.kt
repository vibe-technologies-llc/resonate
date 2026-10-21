package org.resonate.android.ui.components

import android.os.SystemClock
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.expandVertically
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.shrinkVertically
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Close
import androidx.compose.material.icons.rounded.Info
import androidx.compose.material.icons.rounded.Sync
import androidx.compose.material.icons.rounded.LibraryMusic
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearWavyProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.delay
import org.resonate.android.ui.LocalResonate
import org.resonate.android.ui.SyncActivity
import org.resonate.android.ui.counted
import org.resonate.android.ui.syncActivity

@Composable
fun Banners(modifier: Modifier = Modifier) {
    val resonate = LocalResonate.current
    val status by resonate.library.status.collectAsState()
    val scanning by resonate.library.scanning.collectAsState()
    val sync by resonate.features.syncStatus.collectAsState()
    val asked by resonate.syncAskedAt.collectAsState()
    var now by remember { mutableLongStateOf(SystemClock.elapsedRealtime()) }
    LaunchedEffect(asked) {
        while (asked != null) {
            now = SystemClock.elapsedRealtime()
            delay(1_000)
        }
    }
    val activity = syncActivity(sync, asked, now)
    Column(modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        AnimatedVisibility(
            scanning,
            enter = expandVertically() + fadeIn(),
            exit = shrinkVertically() + fadeOut(),
        ) {
            ProgressBanner(Icons.Rounded.LibraryMusic, "Reading music folder", null)
        }
        AnimatedVisibility(
            activity != null,
            enter = expandVertically() + fadeIn(),
            exit = shrinkVertically() + fadeOut(),
        ) {
            when (activity) {
                is SyncActivity.Fetching ->
                    ProgressBanner(
                        Icons.Rounded.Sync,
                        "Fetching songs from your devices",
                        activity.fraction,
                        detail = counted(activity.songs, "song"),
                    )
                else -> ProgressBanner(Icons.Rounded.Sync, "Syncing with your devices", null)
            }
        }
        AnimatedVisibility(
            status != null,
            enter = expandVertically() + fadeIn(),
            exit = shrinkVertically() + fadeOut(),
        ) {
            NoticeBanner(status.orEmpty(), onDismiss = resonate.library::dismissStatus)
        }
    }
}

@Composable
private fun ProgressBanner(icon: ImageVector, label: String, fraction: Float?, detail: String? = null) {
    Surface(
        color = MaterialTheme.colorScheme.secondaryContainer,
        contentColor = MaterialTheme.colorScheme.onSecondaryContainer,
        shape = MaterialTheme.shapes.large,
        modifier =
            Modifier.fillMaxWidth().padding(horizontal = 12.dp).semantics {
                liveRegion = LiveRegionMode.Polite
                contentDescription = label
            },
    ) {
        Column(Modifier.padding(horizontal = 16.dp, vertical = 12.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Icon(icon, null, modifier = Modifier.size(20.dp))
                Spacer(Modifier.width(12.dp))
                Text(
                    label,
                    style = MaterialTheme.typography.labelLarge,
                    modifier = Modifier.weight(1f).clearAndSetSemantics {},
                )
                if (detail != null) {
                    Text(
                        detail,
                        style = MaterialTheme.typography.labelMedium,
                        modifier = Modifier.clearAndSetSemantics {},
                    )
                }
            }
            Spacer(Modifier.size(8.dp))
            if (fraction == null) {
                LinearWavyProgressIndicator(Modifier.fillMaxWidth().clearAndSetSemantics {})
            } else {
                LinearWavyProgressIndicator(
                    progress = { fraction },
                    modifier = Modifier.fillMaxWidth().clearAndSetSemantics {},
                )
            }
        }
    }
}

@Composable
private fun NoticeBanner(message: String, onDismiss: () -> Unit) {
    Surface(
        color = MaterialTheme.colorScheme.inverseSurface,
        contentColor = MaterialTheme.colorScheme.inverseOnSurface,
        shape = MaterialTheme.shapes.large,
        modifier =
            Modifier.fillMaxWidth().padding(horizontal = 12.dp).semantics {
                liveRegion = LiveRegionMode.Polite
            },
    ) {
        Row(
            Modifier.padding(start = 16.dp, end = 4.dp, top = 4.dp, bottom = 4.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(Icons.Rounded.Info, null, modifier = Modifier.size(20.dp))
            Spacer(Modifier.width(12.dp))
            Text(
                message,
                style = MaterialTheme.typography.bodyMedium,
                maxLines = 4,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f).padding(vertical = 10.dp),
            )
            IconButton(onClick = onDismiss) {
                Icon(Icons.Rounded.Close, contentDescription = "Dismiss", tint = Color.Unspecified)
            }
        }
    }
}
