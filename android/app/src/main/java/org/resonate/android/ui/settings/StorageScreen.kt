package org.resonate.android.ui.settings

import android.text.format.Formatter
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Compress
import androidx.compose.material.icons.rounded.DeleteOutline
import androidx.compose.material.icons.rounded.Storage
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.conflate
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import org.resonate.android.R
import org.resonate.android.RecompressRun
import org.resonate.android.RecompressTo
import org.resonate.android.nativecore.SizedSong
import org.resonate.android.nativecore.SizedSongAfter
import org.resonate.android.nativecore.Track
import org.resonate.android.ui.LocalResonate
import org.resonate.android.ui.Resonate
import org.resonate.android.ui.byline
import org.resonate.android.ui.components.EmptyState
import org.resonate.android.ui.components.LocalSheets
import org.resonate.android.ui.components.Page
import org.resonate.android.ui.components.SectionHeader
import org.resonate.android.ui.components.SongSheetRequest
import org.resonate.android.ui.components.Sizes
import org.resonate.android.ui.components.TrackRow
import org.resonate.android.ui.library.MenuEntry
import org.resonate.android.ui.library.OverflowMenu
import org.resonate.android.ui.library.catalogChanges
import org.resonate.android.ui.resonateViewModel

data class StorageView(
    val songs: List<SizedSong> = emptyList(),
    val reading: Boolean = true,
    val ended: Boolean = false,
    val failed: String? = null,
)

object SizedPages {
    const val PAGE = 100
    const val REREAD_AT_MOST_EVERY_MS = 2_000L

    fun after(songs: List<SizedSong>): SizedSongAfter? =
        songs.lastOrNull()?.let { SizedSongAfter(it.bytes, it.track.id) }

    fun rereadAsking(view: StorageView): Int = maxOf(PAGE, view.songs.size)

    fun reread(page: List<SizedSong>, asked: Int): StorageView =
        StorageView(page.distinctBy { it.track.id }, reading = false, ended = page.size < asked)

    fun appended(view: StorageView, page: List<SizedSong>, asked: Int): StorageView {
        val held = view.songs.mapTo(HashSet()) { it.track.id }
        return view.copy(
            songs = view.songs + page.filter { it.track.id !in held }.distinctBy { it.track.id },
            reading = false,
            ended = page.size < asked,
            failed = null,
        )
    }

    fun failed(view: StorageView, text: String): StorageView = view.copy(reading = false, failed = text)
}

class StorageViewModel(private val resonate: Resonate) : ViewModel() {
    private val mutableView = MutableStateFlow(StorageView())
    private val reading = Mutex()

    val view: StateFlow<StorageView> = mutableView.asStateFlow()
    val to: StateFlow<RecompressTo> = resonate.features.recompressing.to
    val run: StateFlow<RecompressRun?> = resonate.features.recompressing.run

    init {
        viewModelScope.launch {
            resonate.catalogChanges().conflate().collect {
                reread()
                delay(SizedPages.REREAD_AT_MOST_EVERY_MS)
            }
        }
    }

    fun more() {
        val shown = mutableView.value
        if (shown.ended || shown.reading || shown.failed != null) return
        mutableView.value = shown.copy(reading = true)
        viewModelScope.launch {
            reading.withLock {
                val after = SizedPages.after(mutableView.value.songs)
                read(after, SizedPages.PAGE)
                    .onSuccess { mutableView.value = SizedPages.appended(mutableView.value, it, SizedPages.PAGE) }
                    .onFailure { failed(it) }
            }
        }
    }

    fun again() {
        mutableView.value = StorageView()
        viewModelScope.launch { reread() }
    }

    fun choose(to: RecompressTo) = resonate.features.recompressing.choose(to)

    fun recompress(track: Track) = resonate.recompress(track)

    fun recompressEvery() = resonate.recompressEvery()

    fun stop() = resonate.features.recompressing.stop()

    fun recompressible(track: Track): Boolean = resonate.features.recompressing.recompressible(track)

    fun deletable(track: Track): Boolean = resonate.features.songDeletable(track)

    fun everywhere(): Boolean = resonate.features.syncsWithOtherDevices()

    private suspend fun reread() =
        reading.withLock {
            val asked = SizedPages.rereadAsking(mutableView.value)
            read(null, asked)
                .onSuccess { mutableView.value = SizedPages.reread(it, asked) }
                .onFailure { failed(it) }
        }

    private suspend fun read(after: SizedSongAfter?, limit: Int): Result<List<SizedSong>> =
        resonate.read { native -> withContext(Dispatchers.IO) { native.largestSongs(after, limit.toUInt()) } }

    private fun failed(failure: Throwable) {
        mutableView.value =
            SizedPages.failed(
                mutableView.value,
                failure.message ?: resonate.library.words.string(R.string.storage_failed),
            )
    }
}

@Composable
fun StorageScreen() {
    val model = resonateViewModel { StorageViewModel(it) }
    val view by model.view.collectAsStateWithLifecycle()
    val to by model.to.collectAsStateWithLifecycle()
    val run by model.run.collectAsStateWithLifecycle()
    val catalog by LocalResonate.current.library.allTracks.collectAsStateWithLifecycle()
    val context = LocalContext.current
    val sheets = LocalSheets.current
    var confirming by rememberSaveable { mutableStateOf(false) }
    val flacs = remember(catalog) { catalog.count(model::recompressible) }
    val choices = RecompressTo.entries.map { Choice(it, stringResource(it.label), stringResource(it.detail)) }
    Page(stringResource(R.string.settings_storage_title)) { padding ->
        when {
            view.songs.isEmpty() && view.reading ->
                Box(Modifier.fillMaxSize().padding(padding), contentAlignment = Alignment.Center) {
                    CircularProgressIndicator()
                }
            view.songs.isEmpty() && view.failed != null ->
                Box(Modifier.fillMaxSize().padding(padding)) {
                    EmptyState(
                        Icons.Rounded.Storage,
                        stringResource(R.string.storage_failed),
                        detail = view.failed,
                        action = stringResource(R.string.storage_try_again),
                        onAction = model::again,
                    )
                }
            view.songs.isEmpty() ->
                Box(Modifier.fillMaxSize().padding(padding)) {
                    EmptyState(
                        Icons.Rounded.Storage,
                        stringResource(R.string.storage_empty),
                        detail = stringResource(R.string.storage_empty_detail),
                    )
                }
            else ->
                SettingsList(padding, Modifier.testTag("storage-list")) {
                    item(key = "to") {
                        ChoiceRow(
                            title = stringResource(R.string.storage_recompress_to),
                            choices = choices,
                            selected = to,
                            onChoose = model::choose,
                            icon = Icons.Rounded.Compress,
                            enabled = run == null,
                            modifier = Modifier.testTag("recompress-to"),
                        )
                    }
                    item(key = "every") {
                        val running = run
                        if (running != null) RecompressProgress(running, model::stop)
                        else
                            SettingRow(
                                title = stringResource(R.string.storage_recompress_every),
                                icon = Icons.Rounded.Compress,
                                supporting = pluralStringResource(R.plurals.storage_recompress_every_detail, flacs, flacs),
                                enabled = flacs > 0,
                                onClick = { confirming = true },
                                modifier = Modifier.testTag("recompress-every"),
                            )
                    }
                    item(key = "largest") { SectionHeader(stringResource(R.string.storage_largest_first)) }
                    items(view.songs, key = { "song-${it.track.id}" }) { sized ->
                        val track = sized.track
                        val size = Formatter.formatShortFileSize(context, sized.bytes.toLong())
                        TrackRow(
                            track,
                            onClick = { sheets.song = SongSheetRequest(track) },
                            supporting =
                                if (track.byline.isEmpty())
                                    stringResource(R.string.storage_song_line_unnamed, size, track.codec)
                                else stringResource(R.string.storage_song_line, size, track.codec, track.byline),
                            trailing = { SongActions(model, track, run == null) },
                            modifier = Modifier.testTag("storage-song-${track.id}"),
                        )
                    }
                    item(key = "end") {
                        if (!view.ended) {
                            LaunchedEffect(view.songs.size) { model.more() }
                            Box(Modifier.fillMaxWidth().padding(16.dp), contentAlignment = Alignment.Center) {
                                if (view.failed != null)
                                    TextButton(onClick = model::more) {
                                        Text(stringResource(R.string.storage_try_again))
                                    }
                                else CircularProgressIndicator()
                            }
                        }
                    }
                }
        }
    }
    if (confirming) {
        ConfirmDialog(
            title = pluralStringResource(R.plurals.storage_recompress_every_title, flacs, flacs),
            text = stringResource(R.string.storage_recompress_every_text, stringResource(to.label)),
            confirm = stringResource(R.string.storage_recompress),
            icon = Icons.Rounded.Compress,
            onConfirm = model::recompressEvery,
            onDismiss = { confirming = false },
        )
    }
}

@Composable
private fun SongActions(model: StorageViewModel, track: Track, idle: Boolean) {
    val sheets = LocalSheets.current
    val recompress = stringResource(R.string.storage_recompress)
    val delete = stringResource(R.string.storage_delete)
    val deleteEverywhere = stringResource(R.string.storage_delete_everywhere)
    OverflowMenu(
        listOf(
            listOfNotNull(
                MenuEntry.Action(recompress, Icons.Rounded.Compress) { model.recompress(track) }
                    .takeIf { idle && model.recompressible(track) },
            ),
            listOfNotNull(
                MenuEntry.Action(delete, Icons.Rounded.DeleteOutline, destructive = true) {
                        sheets.delete(listOf(track))
                    }
                    .takeIf { model.deletable(track) },
                MenuEntry.Action(deleteEverywhere, Icons.Rounded.DeleteOutline, destructive = true) {
                        sheets.delete(listOf(track), everywhere = true)
                    }
                    .takeIf { model.deletable(track) && model.everywhere() },
            ),
        )
    )
}

@Composable
private fun RecompressProgress(run: RecompressRun, stop: () -> Unit) {
    Column(Modifier.fillMaxWidth().padding(horizontal = Sizes.gutter, vertical = 12.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(
                if (run.of == 0) stringResource(R.string.storage_recompress_finding)
                else stringResource(R.string.storage_recompressing, run.done + 1, run.of),
                style = MaterialTheme.typography.bodyLarge,
                modifier = Modifier.weight(1f),
            )
            TextButton(onClick = stop, modifier = Modifier.testTag("recompress-stop")) {
                Text(stringResource(R.string.storage_recompress_stop))
            }
        }
        if (run.of == 0) LinearProgressIndicator(Modifier.fillMaxWidth())
        else LinearProgressIndicator(progress = { run.done.toFloat() / run.of }, modifier = Modifier.fillMaxWidth())
    }
}
