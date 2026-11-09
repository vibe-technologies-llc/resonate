package org.resonate.android.ui.library

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.BookmarkAdd
import androidx.compose.material.icons.rounded.Download
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.resonate.android.DownloadJob
import org.resonate.android.DownloadPlan
import org.resonate.android.R
import org.resonate.android.nativecore.ReleaseNotHeld
import org.resonate.android.nativecore.SearchSong
import org.resonate.android.ui.LocalResonate
import org.resonate.android.ui.Resonate
import org.resonate.android.ui.components.Picture
import org.resonate.android.ui.components.SheetRow
import org.resonate.android.ui.components.rememberPicture

const val NOT_HELD_COVER_ALPHA = 0.5f

const val SONGS_LACKED_FOLDED = 20

fun releaseLine(release: ReleaseNotHeld): String =
    listOf(release.kind, release.year?.toString().orEmpty()).filter { it.isNotBlank() }.joinToString(" · ")

fun <T> firstOf(items: List<T>, all: Boolean, folded: Int = SONGS_LACKED_FOLDED): List<T> =
    if (all || items.size <= folded) items else items.take(folded)

@Composable
fun UnheldReleaseCard(release: ReleaseNotHeld, onClick: () -> Unit, modifier: Modifier = Modifier) {
    val resonate = LocalResonate.current
    val sleeve by
        produceState<ByteArray?>(null, release.group, release.covered) {
            value =
                if (release.covered) {
                    resonate
                        .read { native ->
                            withContext(Dispatchers.IO) { native.unheldReleaseCover(release.group) }
                        }
                        .getOrNull()
                } else {
                    null
                }
        }
    val notInYourLibrary = stringResource(R.string.library_not_in_your_library)
    CoverCard(
        title = release.title.ifBlank { stringResource(R.string.library_untitled) },
        detail = releaseLine(release).ifBlank { null },
        onClick = onClick,
        modifier =
            modifier.testTag("not-held-release").semantics { stateDescription = notInYourLibrary },
    ) {
        Picture(
            rememberPicture(sleeve),
            Modifier.fillMaxWidth().aspectRatio(1f).alpha(NOT_HELD_COVER_ALPHA),
            shape = MaterialTheme.shapes.large,
        )
    }
}

@Composable
fun UnheldReleaseSheet(
    resonate: Resonate,
    artist: String,
    release: ReleaseNotHeld,
    jobs: List<DownloadJob>,
    onDownload: (List<SearchSong>) -> Unit,
    dismiss: () -> Unit,
) {
    var songs by remember { mutableStateOf<List<SearchSong>?>(null) }
    var failed by remember { mutableStateOf(false) }
    var asked by remember { mutableIntStateOf(0) }
    LaunchedEffect(release.group, asked) {
        failed = false
        val read =
            resonate.read { native ->
                withContext(Dispatchers.IO) {
                    if (!release.songsRead) native.learnReleaseSongs(release.group)
                    native.releaseSongsNotHeld(artist, release.group)
                }
            }
        read.onSuccess { songs = it }.onFailure { failed = true }
    }
    ModalBottomSheet(onDismissRequest = dismiss) {
        Column(
            Modifier.verticalScroll(rememberScrollState())
                .navigationBarsPadding()
                .testTag("not-held-release-sheet")
        ) {
            Text(
                release.title.ifBlank { stringResource(R.string.library_untitled) },
                style = MaterialTheme.typography.titleLarge,
                modifier = Modifier.padding(horizontal = 24.dp),
            )
            Text(
                releaseLine(release),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(horizontal = 24.dp),
            )
            Spacer(Modifier.height(8.dp))
            val listed = songs
            when {
                failed ->
                    TextButton(onClick = { asked++ }, modifier = Modifier.padding(horizontal = 16.dp)) {
                        Text(stringResource(R.string.library_release_unreadable))
                    }
                listed == null ->
                    CircularProgressIndicator(Modifier.padding(24.dp))
                listed.isEmpty() ->
                    Text(
                        stringResource(R.string.library_release_complete),
                        modifier = Modifier.padding(horizontal = 24.dp, vertical = 16.dp),
                    )
                else -> {
                    SheetRow(
                        Icons.Rounded.Download,
                        pluralStringResource(R.plurals.library_release_download, listed.size, listed.size),
                    ) {
                        dismiss()
                        onDownload(listed)
                    }
                    SheetRow(
                        Icons.Outlined.BookmarkAdd,
                        pluralStringResource(R.plurals.library_release_save, listed.size, listed.size),
                    ) {
                        dismiss()
                        resonate.features.saveWanted(listed)
                    }
                    HorizontalDivider(Modifier.padding(vertical = 8.dp))
                    listed.forEach { song ->
                        NotHeldSongRow(
                            song,
                            standing = DownloadPlan.standing(jobs, song),
                            onDownload = {
                                dismiss()
                                onDownload(listOf(song))
                            },
                            onMore = null,
                            number = song.position?.toString().orEmpty(),
                        )
                    }
                }
            }
            Spacer(Modifier.height(16.dp))
        }
    }
}
