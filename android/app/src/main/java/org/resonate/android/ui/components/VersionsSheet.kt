package org.resonate.android.ui.components

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.Undo
import androidx.compose.material.icons.rounded.PlayArrow
import androidx.compose.material.icons.rounded.Search
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.testTagsAsResourceId
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.resonate.android.R
import org.resonate.android.nativecore.FoundVersion
import org.resonate.android.nativecore.FoundVersions
import org.resonate.android.nativecore.Track
import org.resonate.android.nativecore.VersionSearch
import org.resonate.android.nativecore.VersionSearchOffered
import org.resonate.android.ui.LocalResonate
import org.resonate.android.ui.Resonate
import org.resonate.android.ui.shownArtist
import org.resonate.android.ui.shownTitle

sealed interface VersionsShown {
    data object Reading : VersionsShown

    data class Offered(val searches: List<VersionSearchOffered>, val chosen: Boolean) : VersionsShown

    data class Looking(val search: VersionSearchOffered) : VersionsShown

    data class Found(val found: FoundVersions) : VersionsShown
}

@Composable
fun VersionsSheet(track: Track, dismiss: () -> Unit) {
    val resonate = LocalResonate.current
    val scope = rememberCoroutineScope()
    var shown by remember(track.id) { mutableStateOf<VersionsShown>(VersionsShown.Reading) }
    LaunchedEffect(track.id) {
        resonate
            .read { native ->
                withContext(Dispatchers.IO) {
                    VersionsShown.Offered(
                        native.versionSearches(track.id),
                        native.versionChosen(track.id) != null,
                    )
                }
            }
            .onSuccess { shown = it }
            .onFailure { failure ->
                if (failure is Exception) resonate.library.report(failure)
                dismiss()
            }
    }
    ModalBottomSheet(onDismissRequest = dismiss) {
        Column(
            Modifier.verticalScroll(rememberScrollState()).navigationBarsPadding().semantics {
                testTagsAsResourceId = true
            }
        ) {
            ListItem(
                colors = ListItemDefaults.colors(containerColor = Color.Transparent),
                leadingContent = { Artwork(track, Sizes.rowArt, shape = MaterialTheme.shapes.medium) },
                supportingContent = { Text(track.shownArtist, maxLines = 1) },
                overlineContent = { Text(stringResource(R.string.sheet_find_another_version)) },
            ) {
                Text(
                    track.shownTitle,
                    style = MaterialTheme.typography.titleMedium,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            HorizontalDivider(Modifier.padding(vertical = 8.dp))
            when (val now = shown) {
                VersionsShown.Reading ->
                    LinearProgressIndicator(Modifier.fillMaxWidth().padding(24.dp))
                is VersionsShown.Offered -> {
                    now.searches.forEach { offered ->
                        SheetRow(Icons.Rounded.Search, offered.label) {
                            shown = VersionsShown.Looking(offered)
                            scope.launch {
                                shown =
                                    resonate.findVersions(track, offered.search)?.let {
                                        VersionsShown.Found(it)
                                    } ?: now
                            }
                        }
                    }
                    if (now.chosen) {
                        SheetRow(
                            Icons.AutoMirrored.Rounded.Undo,
                            stringResource(R.string.versions_play_own_copy),
                        ) {
                            dismiss()
                            resonate.playOwnCopy(track, scope)
                        }
                    }
                }
                is VersionsShown.Looking -> {
                    Said(stringResource(R.string.versions_looking))
                    LinearProgressIndicator(
                        Modifier.fillMaxWidth().padding(24.dp).testTag("looking-for-versions")
                    )
                }
                is VersionsShown.Found -> {
                    now.found.trouble?.let { Said(it, MaterialTheme.colorScheme.error) }
                    now.found.said?.let { Said(it) }
                    now.found.versions.forEach { version ->
                        SheetRow(Icons.Rounded.PlayArrow, version.label) {
                            dismiss()
                            resonate.playVersion(track, version, scope)
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun Said(words: String, ink: Color = MaterialTheme.colorScheme.onSurfaceVariant) {
    Text(
        words,
        color = ink,
        style = MaterialTheme.typography.bodyMedium,
        modifier =
            Modifier.padding(horizontal = 24.dp, vertical = 8.dp).semantics {
                liveRegion = LiveRegionMode.Polite
            },
    )
}

private suspend fun Resonate.findVersions(track: Track, search: VersionSearch): FoundVersions? =
    read { native ->
            features.served.bind(native)
            withContext(Dispatchers.IO) { native.findVersions(track.id, search) }
        }
        .onFailure { if (it is Exception) library.report(it) }
        .getOrNull()

private fun Resonate.playVersion(
    track: Track,
    version: FoundVersion,
    scope: CoroutineScope,
) {
    attempt(scope) { native ->
        val said = withContext(Dispatchers.IO) { native.chooseVersion(track.id, version.version) }
        playback.play(listOf(track))
        library.notice(said)
    }
}

private fun Resonate.playOwnCopy(track: Track, scope: CoroutineScope) {
    attempt(scope) { native ->
        val said = withContext(Dispatchers.IO) { native.forgetVersionChosen(track.id) }
        playback.play(listOf(track))
        library.notice(said)
    }
}
