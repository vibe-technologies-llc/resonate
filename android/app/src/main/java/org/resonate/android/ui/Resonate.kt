package org.resonate.android.ui

import android.net.Uri
import android.os.SystemClock
import android.util.Log
import androidx.compose.runtime.Composable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.media3.common.Player
import androidx.media3.session.MediaController
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.resonate.android.LibraryFeatures
import org.resonate.android.LibraryRepository
import org.resonate.android.mediaItem
import org.resonate.android.nativecore.MobilePlayer
import org.resonate.android.nativecore.Track

interface Host {
    fun chooseFolder()

    fun chooseRoomResponse()

    fun useDownloads()

    fun allowNotifications()

    fun share(title: String, link: String)

    fun open(link: Uri)

    suspend fun confirmDelete(uris: List<Uri>): Boolean

    suspend fun confirmWrite(uris: List<Uri>): Boolean

    suspend fun allowMicrophone(): Boolean

    fun openAppSettings()
}

enum class Repeat {
    OFF,
    ONE,
    ALL,
}

class Playback(
    private val library: LibraryRepository,
    private val controller: () -> MediaController?,
) {
    private fun remote(): MediaController? =
        controller().also { if (it == null) library.notice("Playback is still connecting") }

    fun play(tracks: List<Track>, index: Int = 0) {
        val remote = remote() ?: return
        if (tracks.isEmpty()) return
        remote.setMediaItems(tracks.map { it.mediaItem() }, index.coerceIn(tracks.indices), 0)
        remote.prepare()
        remote.play()
    }

    fun toggle() {
        val remote = remote() ?: return
        if (remote.playWhenReady) remote.pause() else remote.play()
    }

    fun resume() {
        remote()?.play()
    }

    fun previous() {
        remote()?.seekToPreviousMediaItem()
    }

    fun next() {
        remote()?.seekToNextMediaItem()
    }

    fun seek(positionMs: Long) {
        remote()?.seekTo(positionMs)
    }

    fun jump(index: Int) {
        remote()?.seekToDefaultPosition(index)
    }

    fun shuffle(enabled: Boolean) {
        remote()?.shuffleModeEnabled = enabled
    }

    fun repeat(mode: Repeat) {
        remote()?.repeatMode =
            when (mode) {
                Repeat.OFF -> Player.REPEAT_MODE_OFF
                Repeat.ONE -> Player.REPEAT_MODE_ONE
                Repeat.ALL -> Player.REPEAT_MODE_ALL
            }
    }

    fun volume(value: Float) {
        remote()?.volume = value.coerceIn(0f, 1f)
    }
}

class Resonate(val library: LibraryRepository, val playback: Playback, val host: Host) {
    val features: LibraryFeatures
        get() = library.features

    val syncAskedAt = MutableStateFlow<Long?>(null)

    fun listen() = features.listening.listen(host::allowMicrophone)

    fun keepListening(keeps: Boolean) = features.listening.keepListening(keeps, host::allowMicrophone)

    fun deleteSongs(tracks: List<Track>) = features.deleteSongs(tracks, host::confirmDelete)

    fun editTags(tracks: List<Track>, change: org.resonate.android.nativecore.SongTags) =
        features.tags.edit(tracks, change, host::confirmWrite)

    fun undoTags() = features.tags.undo(host::confirmWrite)

    suspend fun deleteCopies(sets: List<org.resonate.android.nativecore.IdenticalSongs>) =
        features.deleteCopies(sets, host::confirmDelete)

    fun forceSync(scope: CoroutineScope): Job {
        syncAskedAt.value = SystemClock.elapsedRealtime()
        return attempt(scope) { native -> withContext(Dispatchers.IO) { native.syncForce() } }
    }

    fun attempt(scope: CoroutineScope, action: suspend (MobilePlayer) -> Unit): Job =
        scope.launch {
            try {
                action(library.native())
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (failure: Exception) {
                library.report(failure)
            } catch (failure: LinkageError) {
                Log.e("Resonate", "A native call failed", failure)
                library.notice("The action could not be completed")
            }
        }

    suspend fun <T> read(action: suspend (MobilePlayer) -> T): Result<T> =
        try {
            Result.success(action(library.native()))
        } catch (cancelled: CancellationException) {
            throw cancelled
        } catch (failure: Exception) {
            Result.failure(failure)
        } catch (failure: LinkageError) {
            Log.e("Resonate", "A native call failed", failure)
            Result.failure(IllegalStateException("The action could not be completed", failure))
        }
}

val LocalResonate = staticCompositionLocalOf<Resonate> { error("No Resonate in composition") }

@Composable
inline fun <reified T : ViewModel> resonateViewModel(
    key: String? = null,
    crossinline make: (Resonate) -> T,
): T {
    val resonate = LocalResonate.current
    return viewModel(key = key) { make(resonate) }
}
