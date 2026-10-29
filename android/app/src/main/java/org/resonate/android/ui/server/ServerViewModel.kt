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
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.flatMapLatest
import kotlinx.coroutines.flow.flowOf
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn
import org.resonate.android.chosenService
import org.resonate.android.nativecore.ServedAlbum
import org.resonate.android.nativecore.ServedFound
import org.resonate.android.nativecore.ServedService
import org.resonate.android.nativecore.ServedSong
import org.resonate.android.ui.Resonate
import org.resonate.android.ui.library.Loaded
import org.resonate.android.ui.library.Retry

private val shared = SharingStarted.WhileSubscribed(5_000)

@OptIn(ExperimentalCoroutinesApi::class)
class ServerViewModel(resonate: Resonate) : ViewModel() {
    val field = TextFieldState()
    val firstPageRetry = Retry()
    val searchRetry = Retry()
    private val asked = MutableStateFlow("")
    private val picked = MutableStateFlow<String?>(null)

    val services: StateFlow<List<ServedService>> = resonate.features.served.services

    val chosen: StateFlow<ServedService?> =
        combine(services, picked, ::chosenService)
            .distinctUntilChanged()
            .stateIn(viewModelScope, shared, chosenService(services.value, null))

    private val scheme = chosen.map { it?.scheme }.distinctUntilChanged()

    val typed: StateFlow<Boolean> =
        snapshotFlow { field.text.isNotEmpty() }.stateIn(viewModelScope, shared, false)

    val firstPage: StateFlow<Loaded<ServedFound>> =
        scheme
            .flatMapLatest { service ->
                if (service == null) flowOf<Loaded<ServedFound>>(Loaded.Ready(nothingFound))
                else resonate.servedRead(firstPageRetry) { it.servedFirstPage(service) }
            }
            .stateIn(viewModelScope, shared, Loaded.Loading)

    val found: StateFlow<Loaded<ServedFound>?> =
        combine(asked, scheme) { words, service -> words to service }
            .flatMapLatest { (words, service) ->
                if (words.isEmpty() || service == null) flowOf<Loaded<ServedFound>?>(null)
                else resonate.servedRead(searchRetry) { it.servedSearch(service, words) }
            }
            .stateIn(viewModelScope, shared, null)

    fun choose(service: ServedService) {
        picked.value = service.scheme
    }

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
