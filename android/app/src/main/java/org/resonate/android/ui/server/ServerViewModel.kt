package org.resonate.android.ui.server

import androidx.compose.foundation.text.input.TextFieldState
import androidx.compose.foundation.text.input.clearText
import androidx.compose.runtime.snapshotFlow
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.flatMapLatest
import kotlinx.coroutines.flow.flowOf
import kotlinx.coroutines.flow.stateIn
import org.resonate.android.nativecore.ServedAlbum
import org.resonate.android.nativecore.ServedArtist
import org.resonate.android.nativecore.ServedFound
import org.resonate.android.nativecore.ServedSong
import org.resonate.android.ui.Resonate
import org.resonate.android.ui.library.Loaded
import org.resonate.android.ui.library.Retry

private val shared = SharingStarted.WhileSubscribed(5_000)

@OptIn(ExperimentalCoroutinesApi::class)
class ServerViewModel(resonate: Resonate) : ViewModel() {
    val field = TextFieldState()
    val artistsRetry = Retry()
    val searchRetry = Retry()
    private val asked = MutableStateFlow("")

    val typed: StateFlow<Boolean> =
        snapshotFlow { field.text.isNotEmpty() }.stateIn(viewModelScope, shared, false)

    val artists: StateFlow<Loaded<List<ServedArtist>>> =
        resonate.servedRead(artistsRetry) { it.servedArtists() }.stateIn(viewModelScope, shared, Loaded.Loading)

    val found: StateFlow<Loaded<ServedFound>?> =
        asked
            .flatMapLatest { words ->
                if (words.isEmpty()) flowOf<Loaded<ServedFound>?>(null)
                else resonate.servedRead(searchRetry) { it.servedSearch(words) }
            }
            .stateIn(viewModelScope, shared, null)

    fun submit() {
        asked.value = field.text.toString().trim()
    }

    fun clear() {
        field.clearText()
        asked.value = ""
    }
}

class ServedAlbumsViewModel(resonate: Resonate, uri: String) : ViewModel() {
    val retry = Retry()

    val albums: StateFlow<Loaded<List<ServedAlbum>>> =
        resonate.servedRead(retry) { it.servedAlbums(uri) }.stateIn(viewModelScope, shared, Loaded.Loading)
}

class ServedSongsViewModel(resonate: Resonate, uri: String) : ViewModel() {
    val retry = Retry()

    val songs: StateFlow<Loaded<List<ServedSong>>> =
        resonate.servedRead(retry) { it.servedSongs(uri) }.stateIn(viewModelScope, shared, Loaded.Loading)
}
