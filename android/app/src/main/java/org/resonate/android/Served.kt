package org.resonate.android

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.util.Log
import android.util.LruCache
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
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
import org.resonate.android.nativecore.DownloadAccount
import org.resonate.android.nativecore.MobilePlayer
import org.resonate.android.nativecore.MusicProvider
import org.resonate.android.nativecore.ServedService
import org.resonate.android.nativecore.Track

val SERVED_SCHEMES = listOf("subsonic", "tidal", "resonate", "hifi-api", "monochrome")
private const val COVERS_AT_ONCE = 4
private const val COVER_BYTES_AT_MOST = 24 * 1024 * 1024
private const val MISSING_COVERS_AT_MOST = 2048
private const val LARGEST_SIDE = 32768

fun isServedUri(uri: String): Boolean = uri.substringBefore(':', "") in SERVED_SCHEMES

val Track.served: Boolean
    get() = id == 0uL && isServedUri(uri)

val Track.pictureKey: String
    get() = if (served) uri else id.toString()

val Track.rowKey: ULong
    get() = if (served) queueId ?: 0uL else id

fun List<ServedService>.marksFavouritesOf(track: Track): Boolean =
    any { it.scheme == track.uri.substringBefore(':', "") && it.favourites }

fun servedMediaId(uri: String): String? = uri.takeIf(::isServedUri)

private val DownloadAccount.hosted: Boolean
    get() =
        (provider == MusicProvider.MONOCHROME || provider == MusicProvider.HIFI_API) && server.isBlank()

fun ProviderAccountBook?.servedAccounts(online: Boolean, hostedServices: Boolean): List<DownloadAccount> =
    if (!online || this == null) emptyList() else asked.filter { hostedServices || !it.hosted }

fun chosenService(services: List<ServedService>, picked: String?): ServedService? =
    services.firstOrNull { it.scheme == picked } ?: services.firstOrNull()

class Served(
    private val library: LibraryRepository,
    private val scope: CoroutineScope,
    online: StateFlow<Boolean>,
    book: StateFlow<ProviderAccountBook?>,
    hostedServices: StateFlow<Boolean>,
) {
    val accounts: StateFlow<List<DownloadAccount>> =
        combine(online, book, hostedServices) { on, kept, hosted -> kept.servedAccounts(on, hosted) }
            .distinctUntilChanged()
            .stateIn(scope, SharingStarted.Eagerly, emptyList())

    private val given = MutableStateFlow<List<ServedService>>(emptyList())
    val services: StateFlow<List<ServedService>> = given

    val serving: StateFlow<Boolean> =
        services.map { it.isNotEmpty() }.stateIn(scope, SharingStarted.Eagerly, false)

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
                accounts.collect { asked ->
                    forgetCovers()
                    given.value =
                        withContext(Dispatchers.IO) {
                            native.serveFrom(asked)
                            native.servedServices()
                        }
                }
            }
    }

    fun bind(native: MobilePlayer) = native.serveFrom(accounts.value)

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
