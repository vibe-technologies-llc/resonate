package org.resonate.android

import android.content.Context
import android.net.Uri
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import org.resonate.android.nativecore.MobilePlayer
import org.resonate.android.nativecore.RoomCorrection

private const val ON_KEY = "room-correction"
private const val NAME_KEY = "room-correction-name"

val RESPONSE_DOCUMENTS =
    arrayOf("audio/*", "application/x-hdf5", "application/x-netcdf", "application/octet-stream")
private const val RESPONSE_BYTES_AT_MOST = 64 * 1024 * 1024

data class RoomView(
    val on: Boolean,
    val name: String?,
    val kept: RoomCorrection?,
    val reading: Boolean,
    val left: RoomHalf? = null,
)

data class RoomHalf(val uri: Uri, val name: String)

fun roomStem(name: String): String = name.substringBeforeLast('.')

fun roomExtension(name: String): String = name.substringAfterLast('.', "")

fun pairedRoomName(left: String, right: String): String = "${roomStem(left)} and ${roomStem(right)}"

class RoomCorrectionState(
    context: Context,
    private val library: LibraryRepository,
    private val scope: CoroutineScope,
    private val resolver: android.content.ContentResolver,
) {
    private val settings = context.getSharedPreferences("playback", Context.MODE_PRIVATE)
    private val access = Mutex()
    val view =
        MutableStateFlow(
            RoomView(
                on = settings.getBoolean(ON_KEY, false),
                name = settings.getString(NAME_KEY, null),
                kept = null,
                reading = false,
            )
        )

    fun restore(native: MobilePlayer) = step {
        try {
            view.value = view.value.copy(kept = native.keptRoomCorrection())
            if (view.value.on && native.applyRoomCorrection(true) == null) gone()
        } catch (failure: Exception) {
            remember(false)
            throw failure
        }
    }

    fun turn(on: Boolean) = step { apply(library.native(), on) }

    fun choose(uri: Uri) {
        view.value = view.value.copy(reading = true)
        step {
            try {
                val native = library.native()
                val name = resolver.displayNameOf(uri)
                val bytes = withContext(Dispatchers.IO) { read(uri) }
                val kept = native.keepRoomCorrection(bytes, roomExtension(name))
                val stem = roomStem(name)
                settings.edit().putString(NAME_KEY, stem).apply()
                view.value = view.value.copy(kept = kept, name = stem)
                apply(native, true)
            } finally {
                view.value = view.value.copy(reading = false)
            }
        }
    }

    fun chooseLeft(uri: Uri) = step {
        view.value = view.value.copy(left = RoomHalf(uri, resolver.displayNameOf(uri)))
    }

    fun chooseRight(uri: Uri) {
        val left = view.value.left ?: return
        view.value = view.value.copy(reading = true)
        step {
            try {
                val native = library.native()
                val right = resolver.displayNameOf(uri)
                val leftBytes = withContext(Dispatchers.IO) { read(left.uri) }
                val rightBytes = withContext(Dispatchers.IO) { read(uri) }
                val kept =
                    native.keepBinauralRoomCorrection(
                        leftBytes,
                        roomExtension(left.name),
                        rightBytes,
                        roomExtension(right),
                    )
                val named = pairedRoomName(left.name, right)
                settings.edit().putString(NAME_KEY, named).apply()
                view.value = view.value.copy(kept = kept, name = named)
                apply(native, true)
            } finally {
                view.value = view.value.copy(reading = false, left = null)
            }
        }
    }

    fun cancelPair() {
        view.value = view.value.copy(left = null)
    }

    fun forget() = step {
        library.native().forgetRoomCorrection()
        settings.edit().remove(NAME_KEY).apply()
        view.value = view.value.copy(kept = null, name = null)
        remember(false)
    }

    private fun step(action: suspend () -> Unit) {
        scope.launch {
            access.withLock {
                try {
                    action()
                } catch (cancelled: CancellationException) {
                    throw cancelled
                } catch (failure: Exception) {
                    library.report(failure)
                }
            }
        }
    }

    private fun apply(native: MobilePlayer, wanted: Boolean) {
        val applied = native.applyRoomCorrection(wanted)
        view.value = view.value.copy(kept = applied ?: view.value.kept)
        remember(wanted && applied != null)
        if (wanted && applied == null) gone()
    }

    private fun gone() {
        remember(false)
        library.notice("The room response is no longer kept, so room correction stays off")
    }

    private fun remember(on: Boolean) {
        settings.edit().putBoolean(ON_KEY, on).apply()
        view.value = view.value.copy(on = on)
    }

    private fun read(uri: Uri): ByteArray =
        resolver.openInputStream(uri)?.use { it.readNBytes(RESPONSE_BYTES_AT_MOST + 1) }
            ?: ByteArray(0)
}
