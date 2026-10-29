package org.resonate.android

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.util.Log
import android.util.LruCache
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Semaphore
import kotlinx.coroutines.sync.withPermit
import kotlinx.coroutines.withContext
import org.resonate.android.nativecore.MobilePlayer
import org.resonate.android.nativecore.MusicProvider
import org.resonate.android.nativecore.ServedAccount
import org.resonate.android.nativecore.Track

private const val SERVED_SCHEME = "subsonic:"
private const val COVERS_AT_ONCE = 4
private const val COVER_BYTES_AT_MOST = 24 * 1024 * 1024
private const val MISSING_COVERS_AT_MOST = 2048
private const val LARGEST_SIDE = 32768

val Track.served: Boolean
    get() = id == 0uL && uri.startsWith(SERVED_SCHEME)

val Track.pictureKey: String
    get() = if (served) uri else id.toString()

val Track.rowKey: ULong
    get() = if (served) queueId ?: 0uL else id

fun servedMediaId(uri: String): String? = uri.takeIf { it.startsWith(SERVED_SCHEME) }

fun ProviderAccountBook.servedAccount(): ServedAccount? =
    asked
        .firstOrNull { it.provider == MusicProvider.SUBSONIC }
        ?.let { ServedAccount(it.server, it.user, it.password) }

class Served(
    private val library: LibraryRepository,
    private val scope: CoroutineScope,
    online: StateFlow<Boolean>,
    accounts: StateFlow<ProviderAccountBook?>,
) {
    val account: StateFlow<ServedAccount?> =
        combine(online, accounts) { on, book -> if (on) book?.servedAccount() else null }
            .distinctUntilChanged()
            .stateIn(scope, SharingStarted.Eagerly, null)

    val serving: StateFlow<Boolean> =
        account.map { it != null }.stateIn(scope, SharingStarted.Eagerly, false)

    private var applying: Job? = null
    private val slots = Semaphore(COVERS_AT_ONCE)
    private val pictures =
        object : LruCache<String, Bitmap>(COVER_BYTES_AT_MOST) {
            override fun sizeOf(key: String, value: Bitmap) = value.byteCount
        }
    private val missing =
        object : LinkedHashMap<String, Unit>() {
            override fun removeEldestEntry(eldest: MutableMap.MutableEntry<String, Unit>) =
                size > MISSING_COVERS_AT_MOST
        }

    fun begin(native: MobilePlayer) {
        applying?.cancel()
        applying =
            scope.launch {
                account.collect { given ->
                    forgetCovers()
                    native.serveFrom(given)
                }
            }
    }

    fun bind(native: MobilePlayer) = native.serveFrom(account.value)

    fun cached(uri: String, edge: Int): Bitmap? = pictures.get(coverKey(uri, edge))

    suspend fun bitmap(uri: String, cover: String?, edge: Int): Bitmap? {
        val key = coverKey(uri, edge)
        pictures.get(key)?.let {
            return it
        }
        if (isMissing(key)) return null
        return slots.withPermit {
            withContext(Dispatchers.IO) {
                try {
                    val native = library.native()
                    bind(native)
                    val bytes = native.servedCover(uri, cover, edge.toUInt())
                    val picture = decoded(bytes, edge)
                    if (picture == null) noteMissing(key) else pictures.put(key, picture)
                    picture
                } catch (cancelled: CancellationException) {
                    throw cancelled
                } catch (failure: Exception) {
                    Log.w("Resonate", "A served cover could not be drawn", failure)
                    noteMissing(key)
                    null
                }
            }
        }
    }

    private fun coverKey(uri: String, edge: Int) = "$uri@$edge"

    private fun isMissing(key: String) = synchronized(missing) { missing.containsKey(key) }

    private fun noteMissing(key: String) = synchronized(missing) { missing[key] = Unit }

    private fun forgetCovers() {
        pictures.evictAll()
        synchronized(missing) { missing.clear() }
    }
}

internal fun decoded(bytes: ByteArray, edge: Int): Bitmap? {
    val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
    BitmapFactory.decodeByteArray(bytes, 0, bytes.size, bounds)
    if (
        bounds.outWidth <= 0 ||
            bounds.outHeight <= 0 ||
            bounds.outWidth > LARGEST_SIDE ||
            bounds.outHeight > LARGEST_SIDE
    )
        return null
    val options =
        BitmapFactory.Options().apply {
            inSampleSize = ArtworkSizes.sampleFor(bounds.outWidth, bounds.outHeight, edge)
        }
    return BitmapFactory.decodeByteArray(bytes, 0, bytes.size, options)
}
