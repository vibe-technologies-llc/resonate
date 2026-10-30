package org.resonate.android.ui.server

import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import kotlin.random.Random
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.resonate.android.nativecore.ServedSong
import org.resonate.android.ui.LocalResonate
import org.resonate.android.ui.Resonate

class ServerActions(private val resonate: Resonate, private val scope: CoroutineScope) {
    fun play(uri: String) =
        expanded(uri) { uris ->
            resonate.playback.shuffle(false)
            resonate.playback.playServed(uris)
        }

    fun shuffle(uri: String) =
        expanded(uri) { uris ->
            resonate.playback.playServed(uris, Random.nextInt(uris.size))
            resonate.playback.shuffle(true)
        }

    fun playNext(uri: String) =
        expanded(uri) { uris -> resonate.library.request { it.playServedNext(uris) } }

    fun addToQueue(uri: String) =
        expanded(uri) { uris -> resonate.library.request { it.addServedToQueue(uris) } }

    fun playFrom(songs: List<ServedSong>, index: Int) =
        resonate.playback.playServed(songs.map { it.uri }, index)

    fun favour(uri: String, enabled: Boolean, undo: () -> Unit) {
        resonate.attempt(scope) { native ->
            resonate.features.served.bind(native)
            try {
                withContext(Dispatchers.IO) { native.favourServedSong(uri, enabled) }
            } catch (failure: Exception) {
                undo()
                throw failure
            }
        }
    }

    private fun expanded(uri: String, then: (List<String>) -> Unit) {
        resonate.attempt(scope) { native ->
            resonate.features.served.bind(native)
            val uris = withContext(Dispatchers.IO) { native.servedSongUris(uri) }
            if (uris.isNotEmpty()) then(uris)
        }
    }
}

@Composable
fun rememberServerActions(): ServerActions {
    val resonate = LocalResonate.current
    val scope = rememberCoroutineScope()
    return remember(resonate, scope) { ServerActions(resonate, scope) }
}
