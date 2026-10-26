package org.resonate.android.ui.search

import androidx.activity.compose.LocalActivity
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.BookmarkBorder
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.SnackbarDuration
import androidx.compose.material3.SnackbarResult
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.ViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.lifecycleScope
import androidx.lifecycle.viewModelScope
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.drop
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.resonate.android.nativecore.MobilePlayer
import org.resonate.android.nativecore.SearchSong
import org.resonate.android.nativecore.WantedSong
import org.resonate.android.ui.Resonate
import org.resonate.android.ui.components.EmptyState
import org.resonate.android.ui.components.LocalSheets
import org.resonate.android.ui.components.Page
import org.resonate.android.ui.components.Sizes
import org.resonate.android.ui.components.TitleSize
import org.resonate.android.ui.counted
import org.resonate.android.ui.navigation.LocalNavigator
import org.resonate.android.ui.navigation.Screen
import org.resonate.android.ui.resonateViewModel
import org.resonate.android.ui.settings.SwitchRow

fun wantedSummary(songs: List<WantedSong>): String? {
    if (songs.isEmpty()) return null
    val held = songs.count { it.held }
    val saved = counted(songs.size, "song")
    return if (held == 0) saved else "$saved · $held in your library"
}

class WantedViewModel(private val resonate: Resonate) : ViewModel() {
    private val features = resonate.features
    private val songs = MutableStateFlow<List<WantedSong>?>(null)

    val downloads = Downloads(resonate, viewModelScope)
    val shown: StateFlow<List<WantedSong>?> = songs.asStateFlow()

    init {
        viewModelScope.launch { features.wanted.drop(1).collect { songs.value = it } }
        viewModelScope.launch {
            val read = resonate.read { native -> withContext(Dispatchers.IO) { allWanted(native) } }
            if (songs.value == null) songs.value = read.getOrDefault(features.wanted.value)
        }
    }

    fun remove(song: WantedSong) {
        features.removeWanted(song.id)
    }

    fun saveAgain(song: SearchSong) {
        features.saveWanted(listOf(song))
    }
}

private fun allWanted(native: MobilePlayer): List<WantedSong> {
    val rows = ArrayList<WantedSong>()
    do {
        val page = native.wantedSongs(rows.size.toUInt(), WANTED_PAGE)
        rows.addAll(page)
    } while (page.size.toUInt() == WANTED_PAGE)
    return rows
}

private const val WANTED_PAGE = 500u

@Composable
private fun WantedControls(
    downloads: Downloads,
    missing: List<SearchSong>,
    choose: (List<SearchSong>) -> Unit,
) {
    val automatic by downloads.automatic.collectAsStateWithLifecycle()
    val remembered by downloads.remembered.collectAsStateWithLifecycle()
    val ready by downloads.ready.collectAsStateWithLifecycle()
    Column {
        SwitchRow(
            title = "Download saved songs automatically",
            supporting =
                when {
                    !automatic -> "Saved songs wait until you download them"
                    remembered == null -> "Remember a download folder to start"
                    else -> "Matches are fetched into ${remembered?.name} on an unmetered network while Resonate is open"
                },
            checked = automatic,
            onCheckedChange = downloads::automatically,
            modifier = Modifier.testTag("wants-automatic"),
        )
        if (remembered != null) {
            TextButton(
                onClick = downloads::forgetFolder,
                modifier = Modifier.padding(horizontal = 8.dp).testTag("forget-download-folder"),
            ) {
                Text("Forget ${remembered?.name}")
            }
        }
        if (ready && missing.isNotEmpty()) {
            FilledTonalButton(
                onClick = { downloads.request(missing) { choose(missing) } },
                modifier = Modifier.padding(horizontal = Sizes.gutter).testTag("download-missing"),
            ) {
                Text("Download ${counted(missing.size, "missing song")}")
            }
        }
    }
}

@Composable
fun WantedScreen() {
    val model = resonateViewModel { WantedViewModel(it) }
    val songs by model.shown.collectAsStateWithLifecycle()
    val navigator = LocalNavigator.current
    val sheets = LocalSheets.current
    val scope = rememberCoroutineScope()
    val lasting = (LocalActivity.current as? LifecycleOwner)?.lifecycleScope ?: scope
    var acting by remember { mutableStateOf<WantedSong?>(null) }
    var choosing by remember { mutableStateOf<List<SearchSong>?>(null) }
    LaunchedEffect(Unit) { model.downloads.refresh() }

    Page(
        "Saved for later",
        subtitle = songs?.let(::wantedSummary),
        size = TitleSize.MEDIUM,
    ) { padding ->
        Box(Modifier.fillMaxSize(), contentAlignment = Alignment.TopCenter) {
            LazyColumn(
                contentPadding = padding,
                modifier = Modifier.widthIn(max = ContentWidth).fillMaxSize().testTag("wanted-list"),
            ) {
                item(key = "download") { DownloadCard(model.downloads) }
                item(key = "controls") {
                    WantedControls(
                        model.downloads,
                        songs.orEmpty().filter { !it.held }.map { it.song },
                        choose = { choosing = it },
                    )
                }
                val shown = songs
                if (shown == null) loading()
                else if (shown.isEmpty()) {
                    item(key = "empty") {
                        EmptyState(
                            Icons.Outlined.BookmarkBorder,
                            "Nothing saved for later",
                            Modifier.fillParentMaxHeight(0.8f).padding(bottom = 32.dp),
                            detail = "Save songs from online search and find them here to download later.",
                            action = "Search online",
                            onAction = { navigator.selectAtRoot(Screen.Search) },
                        )
                    }
                }
                items(shown.orEmpty(), key = { it.id.toString() }) { wanted ->
                    OnlineSongRow(
                        song = wanted.song,
                        held = wanted.held,
                        saved = true,
                        onClick = { acting = wanted },
                        modifier = Modifier.animateItem(),
                    )
                }
            }
        }
    }

    acting?.let { wanted ->
        OnlineSongSheet(
            song = wanted.song,
            held = wanted.held,
            dismiss = { acting = null },
            actions =
                wantedSongActions(
                    download = { model.downloads.request(listOf(wanted.song)) { choosing = listOf(wanted.song) } },
                    remove = {
                        model.remove(wanted)
                        lasting.launch {
                            val answer =
                                sheets.snackbar.showSnackbar(
                                    "Removed “${wanted.song.title}”",
                                    actionLabel = "Undo",
                                    duration = SnackbarDuration.Short,
                                )
                            if (answer == SnackbarResult.ActionPerformed) {
                                model.saveAgain(wanted.song)
                            }
                        }
                    },
                ),
        )
    }
    choosing?.let { songs -> DownloadChooser(songs, model.downloads) { choosing = null } }
}
