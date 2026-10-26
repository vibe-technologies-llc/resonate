package org.resonate.android

import android.app.Application
import android.content.Context
import android.net.Uri
import android.os.SystemClock
import android.provider.DocumentsContract
import android.util.Log
import java.io.FileNotFoundException
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineExceptionHandler
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.delay
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import org.resonate.android.nativecore.Documents
import org.resonate.android.nativecore.MobilePlayer
import org.resonate.android.nativecore.OutputFormat
import org.resonate.android.nativecore.PlaybackPreferences
import org.resonate.android.nativecore.Snapshot
import org.resonate.android.nativecore.Track
import org.resonate.android.nativecore.TrackFilter
import org.resonate.android.nativecore.TrackOrder
import org.resonate.android.nativecore.audioExtensions
import org.resonate.android.nativecore.defaultTrackFilter
import org.resonate.android.nativecore.documentNotFound
import org.resonate.android.nativecore.documentRefused
import org.resonate.android.ui.Playback
import org.resonate.android.ui.Resonate

data class Favourited(val id: ULong, val enabled: Boolean)

class ResonateApplication : Application() {
    val library by lazy { LibraryRepository(this) }
    val current by lazy { CurrentActivity() }
    val resonate by lazy { Resonate(library, Playback(library) { current.controller }, current) }
}

@androidx.annotation.OptIn(markerClass = [androidx.media3.common.util.UnstableApi::class])
class LibraryRepository(private val context: Context) {
    private val scope =
        CoroutineScope(
            SupervisorJob() +
                Dispatchers.IO +
                CoroutineExceptionHandler { _, failure ->
                    Log.e("Resonate", "A library task failed", failure)
                    mutableStatus.value = "The player could not complete that action"
                }
        )
    @Volatile private var engine = CompletableDeferred<MobilePlayer>()
    private val actions = Channel<(MobilePlayer) -> Unit>(Channel.UNLIMITED)
    private val access = Mutex()
    private val scanAccess = Mutex()
    private val mutableAllTracks = MutableStateFlow<List<Track>>(emptyList())
    val allTracks = mutableAllTracks.asStateFlow()
    private val mutableTracks = MutableStateFlow<List<Track>>(emptyList())
    private val mutableQueue = MutableStateFlow<List<Track>>(emptyList())
    private val mutableSnapshot = MutableStateFlow<Snapshot?>(null)
    private val mutableStatus = MutableStateFlow<String?>(null)
    private val mutableScanning = MutableStateFlow(false)
    private val mutablePreferences = MutableStateFlow<PlaybackPreferences?>(null)
    private val mutableRoute = MutableStateFlow<OutputRoute?>(null)
    private val mutableFollowsSongRate = MutableStateFlow(false)
    private val mutableBoundRoutes = MutableStateFlow<List<OutputRoute>>(emptyList())
    private val mutableFavourites = MutableSharedFlow<Favourited>(extraBufferCapacity = 64)
    val favourites: SharedFlow<Favourited> = mutableFavourites.asSharedFlow()
    val tracks = mutableTracks.asStateFlow()
    val queue = mutableQueue.asStateFlow()
    val snapshot = mutableSnapshot.asStateFlow()
    val status = mutableStatus.asStateFlow()
    val scanning = mutableScanning.asStateFlow()
    val preferences = mutablePreferences.asStateFlow()
    val route = mutableRoute.asStateFlow()
    val followsSongRate = mutableFollowsSongRate.asStateFlow()
    val boundRoutes = mutableBoundRoutes.asStateFlow()
    private var started = false
    private var owners = 0

    @Synchronized
    fun retain() {
        owners++
        start()
    }

    @Synchronized
    fun release() {
        if (owners > 0) owners--
        if (owners == 0) close()
    }

    internal val retained: Int
        @Synchronized get() = owners

    private var playbackJob: Job? = null
    private var documentScope: CoroutineScope? = null
    private var search = ""
    private val queryRequest = java.util.concurrent.atomic.AtomicLong()
    val filter = MutableStateFlow<TrackFilter?>(null)
    val features = LibraryFeatures(context, this)
    val openedFiles = OpenedFiles(context, this)
    val browse = LibraryBrowse(context, this)

    internal suspend fun native() = engine.await()

    @Volatile private var indexed: Pair<List<Track>, Map<ULong, Track>>? = null

    fun findTrack(id: ULong): Track? {
        val rows = mutableAllTracks.value
        val index =
            indexed?.takeIf { it.first === rows }?.second
                ?: rows.associateBy { it.id }.also { indexed = rows to it }
        return index[id]
            ?: mutableQueue.value.firstOrNull { it.id == id }
            ?: mutableSnapshot.value?.current?.takeIf { it.id == id }
    }

    internal suspend fun refresh() =
        withContext(Dispatchers.IO) {
            val native = engine.await()
            mutableAllTracks.value = pagedTracks(native, defaultTrackFilter()) { true }
            reload()
        }

    private suspend fun pagedTracks(
        native: MobilePlayer,
        query: TrackFilter,
        stillWanted: () -> Boolean,
    ): List<Track> {
        val found = ArrayList<Track>()
        do {
            val page =
                access.withLock { native.filteredTracks(query, found.size.toUInt(), PAGE_ROWS) }
            found.addAll(page)
        } while (page.size == PAGE_ROWS.toInt() && stillWanted())
        return found
    }

    internal fun notice(message: String) {
        mutableStatus.value = message
    }

    fun clearFilter() {
        filter.value = null
        search = ""
        scope.launch { reload() }
    }

    fun setFilter(value: TrackFilter) {
        if (isDefaultFilter(value)) {
            clearFilter()
        } else {
            filter.value = value
            search = value.text
            scope.launch { reload() }
        }
    }

    init {
        scope.launch {
            for (action in actions) {
                try {
                    val native = engine.await()
                    access.withLock { action(native) }
                } catch (cancelled: CancellationException) {
                    currentCoroutineContext().ensureActive()
                } catch (failure: Exception) {
                    report(failure)
                }
            }
        }
    }

    @Synchronized
    fun start() {
        if (started) return
        started = true
        documentScope = CoroutineScope(SupervisorJob(scope.coroutineContext[Job]) + Dispatchers.IO)
        val previous = playbackJob
        val ready = engine.takeUnless { it.isCompleted } ?: CompletableDeferred<MobilePlayer>()
        engine = ready
        playbackJob = scope.launch {
            previous?.join()
            var opened: MobilePlayer? = null
            try {
                Logcat.install()
                val native =
                    MobilePlayer(
                        context.filesDir.absolutePath,
                        object : Documents {
                            override fun openDocument(uri: String): Int =
                                openedFiles.descriptor(uri) ?: openGranted(uri)

                            private fun openGranted(uri: String): Int =
                                try {
                                    context.contentResolver
                                        .openFileDescriptor(Uri.parse(uri), "r")
                                        ?.detachFd() ?: -1
                                } catch (gone: FileNotFoundException) {
                                    documentNotFound()
                                } catch (failure: Exception) {
                                    Log.w("Resonate", "Document grant could not be opened", failure)
                                    documentRefused()
                                }
                        },
                    )
                opened = native
                val store = preferencesStore()
                native.rewriteDocumentsWith(DocumentRewrites(context))
                native.watchOutputMixer(BitPerfectMixer(context))
                mutableFollowsSongRate.value = store.followsSongRate()
                native.setFollowSongRate(mutableFollowsSongRate.value)
                mutableRoute.value = features.routeInUse()
                mutableBoundRoutes.value = store.boundRoutes()
                val preferences = store.read(mutableRoute.value)
                native.configurePlayback(preferences)
                mutablePreferences.value = preferences
                features.initialize(native)
                ready.complete(native)
                try {
                    refresh()
                } catch (cancelled: CancellationException) {
                    throw cancelled
                } catch (failure: Exception) {
                    report(failure)
                }
                poll(native)
            } catch (cancelled: CancellationException) {
                ready.cancel(cancelled)
                throw cancelled
            } catch (failure: Exception) {
                ready.completeExceptionally(failure)
                report(failure)
            } catch (failure: LinkageError) {
                ready.completeExceptionally(
                    IllegalStateException("The audio library could not be loaded", failure)
                )
                Log.e("Resonate", "Native audio library could not be loaded", failure)
                mutableStatus.value = "The audio library could not be loaded"
            } finally {
                withContext(NonCancellable) {
                    access.withLock {
                        opened?.let { native ->
                            try {
                                native.shutdown()
                            } catch (failure: Exception) {
                                report(failure)
                            }
                            native.close()
                        }
                    }
                    mutableSnapshot.value = null
                    mutableQueue.value = emptyList()
                }
                ended(currentCoroutineContext()[Job])
            }
        }
    }

    private suspend fun poll(native: MobilePlayer) {
        val queueReads = QueueReads()
        var saves = 0
        var failing: String? = null
        var waiting: String? = null
        while (currentCoroutineContext().isActive) {
            try {
                access.withLock {
                    val state = native.snapshot()
                    features.tick(native, state)
                    mutableSnapshot.value = state
                    state.failure?.let { mutableStatus.value = it }
                    if (state.waiting != waiting) {
                        if (waiting != null && mutableStatus.value == waiting) {
                            mutableStatus.value = null
                        }
                        waiting = state.waiting
                        state.waiting?.let { mutableStatus.value = it }
                    }
                    val now = SystemClock.elapsedRealtime()
                    if (queueReads.due(state.queueRevision, state.catalogRevision, now)) {
                        mutableQueue.value = native.queue()
                        queueReads.noted(state.queueRevision, state.catalogRevision, now)
                    }
                    if (++saves >= SAVE_EVERY_TICKS) {
                        saves = 0
                        native.save()
                    }
                }
                failing = null
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (failure: Exception) {
                if (failure.message != failing) report(failure)
                failing = failure.message
            }
            delay(POLL_EVERY_MS)
        }
    }

    @Synchronized
    private fun ended(job: Job?) {
        if (playbackJob === job) started = false
    }

    fun request(action: (MobilePlayer) -> Unit) {
        actions.trySend(action)
    }

    fun saveNow() {
        request { native -> native.save() }
    }

    fun setFavourite(id: ULong, enabled: Boolean) {
        request { native ->
            native.favourite(id, enabled)
            fun updated(rows: List<Track>) = rows.map {
                if (it.id == id) it.copy(favourite = enabled) else it
            }
            mutableAllTracks.value = updated(mutableAllTracks.value)
            mutableTracks.value =
                updated(mutableTracks.value).filter {
                    filter.value?.favourites != true || it.favourite
                }
            mutableQueue.value = updated(mutableQueue.value)
            mutableSnapshot.value?.let { state ->
                mutableSnapshot.value =
                    state.copy(
                        current =
                            state.current?.let {
                                if (it.id == id) it.copy(favourite = enabled) else it
                            }
                    )
            }
            features.playlistRows.value = updated(features.playlistRows.value)
            mutableFavourites.tryEmit(Favourited(id, enabled))
            scope.launch { reload() }
        }
    }

    fun updatePreferences(change: (PlaybackPreferences) -> PlaybackPreferences) {
        request { native ->
            val next = change(native.playbackPreferences())
            native.configurePlayback(next)
            mutablePreferences.value = next
            preferencesStore().write(next, mutableRoute.value)
        }
    }

    fun outputRouteChanged(now: OutputRoute?) {
        request { native ->
            if (now == mutableRoute.value) return@request
            mutableRoute.value = now
            followRoute(native)
        }
    }

    fun outputOffersChanged(device: Int, bitPerfect: List<OutputFormat>) {
        request { native -> native.outputOffersChanged(device, bitPerfect) }
    }

    fun followSongRate(follows: Boolean) {
        request { native ->
            native.setFollowSongRate(follows)
            preferencesStore().keepFollowsSongRate(follows)
            mutableFollowsSongRate.value = follows
        }
    }

    fun bindEqualiser(bound: Boolean) {
        request { native ->
            val route = mutableRoute.value ?: return@request
            val store = preferencesStore()
            if (bound) store.bind(route, native.playbackPreferences().equaliser)
            else store.unbind(route)
            mutableBoundRoutes.value = store.boundRoutes()
            if (!bound) followRoute(native)
        }
    }

    fun forgetEqualiser(forgotten: OutputRoute) {
        request { native ->
            val store = preferencesStore()
            store.unbind(forgotten)
            mutableBoundRoutes.value = store.boundRoutes()
            if (forgotten.id == mutableRoute.value?.id) followRoute(native)
            notice("Forgot the curve for ${forgotten.name}")
        }
    }

    private fun followRoute(native: MobilePlayer) {
        val curve = preferencesStore().read(mutableRoute.value).equaliser
        val current = native.playbackPreferences()
        if (curve == current.equaliser) return
        val next = current.copy(equaliser = curve)
        native.configurePlayback(next)
        mutablePreferences.value = next
    }

    private fun preferencesStore() =
        PlaybackPreferencesStore(context.getSharedPreferences("playback", Context.MODE_PRIVATE))

    fun search(text: String) {
        search = text
        scope.launch { reload() }
    }

    private fun isDefaultFilter(target: TrackFilter): Boolean =
        target.text.isEmpty() &&
            target.genre.isEmpty() &&
            target.artist.isEmpty() &&
            target.album.isEmpty() &&
            target.minimumRate == 0u &&
            target.minimumBits == 0u &&
            !target.favourites &&
            !target.unplayed &&
            target.order == TrackOrder.TITLE &&
            !target.descending

    private suspend fun reload() {
        val request = queryRequest.incrementAndGet()
        val currentFilter = filter.value
        val isDefault = currentFilter == null || isDefaultFilter(currentFilter)
        if (isDefault && search.isEmpty()) {
            mutableTracks.value = mutableAllTracks.value
            return
        }
        val query = (currentFilter ?: defaultTrackFilter()).copy(text = search)
        val native = engine.await()
        val found = pagedTracks(native, query) { request == queryRequest.get() }
        if (request == queryRequest.get()) mutableTracks.value = found
    }

    suspend fun <T> afterScans(action: suspend () -> T): T = scanAccess.withLock { action() }

    suspend fun <T> inOrder(action: (MobilePlayer) -> T): T {
        val answer = CompletableDeferred<T>()
        request { native ->
            try {
                answer.complete(action(native))
            } catch (failure: Exception) {
                answer.completeExceptionally(failure)
            }
        }
        return answer.await()
    }

    fun scan(tree: Uri) {
        val workers = documentScope ?: return
        features.refreshFolders()
        workers.launch { scanAccess.withLock { scanFolder(tree) } }
    }

    private suspend fun scanFolder(tree: Uri) {
        mutableScanning.value = true
        mutableStatus.value = "Reading music folder…"
        var indexed = 0
        var failed = 0
        try {
            val native = engine.await()
            if (tree == MusicDocuments.downloads) {
                for ((uri, name, size, modified) in MusicDocuments.downloadedAudio(context)) {
                    currentCoroutineContext().ensureActive()
                    if (
                        name.substringAfterLast('.', "").lowercase(java.util.Locale.ROOT) !in
                            extensions
                    )
                        continue
                    try {
                        native.scanDocument(
                            uri.toString(),
                            name,
                            size.coerceAtLeast(0L).toULong(),
                            modified,
                        )
                        indexed++
                    } catch (failure: Exception) {
                        Log.w("Resonate", "Downloaded audio could not be indexed", failure)
                        failed++
                    }
                    mutableStatus.value = scanSummary(indexed, failed)
                }
            } else {
                val root = DocumentsContract.getTreeDocumentId(tree)
                val pending = ArrayDeque<Pair<String, Int>>()
                pending.add(root to 0)
                while (pending.isNotEmpty()) {
                    val (parent, depth) = pending.removeFirst()
                    val children = DocumentsContract.buildChildDocumentsUriUsingTree(tree, parent)
                    val columns =
                        arrayOf(
                            DocumentsContract.Document.COLUMN_DOCUMENT_ID,
                            DocumentsContract.Document.COLUMN_DISPLAY_NAME,
                            DocumentsContract.Document.COLUMN_MIME_TYPE,
                            DocumentsContract.Document.COLUMN_SIZE,
                            DocumentsContract.Document.COLUMN_LAST_MODIFIED,
                        )
                    val audio = ArrayList<ScannedAudio>()
                    val sidecars = HashMap<String, Uri>()
                    val covers = HashMap<String, Uri>()
                    context.contentResolver.query(children, columns, null, null, null)?.use { cursor
                        ->
                        while (cursor.moveToNext()) {
                            currentCoroutineContext().ensureActive()
                            val id = cursor.getString(0)
                            val name = cursor.getString(1) ?: continue
                            val mime = cursor.getString(2)
                            val uri = DocumentsContract.buildDocumentUriUsingTree(tree, id)
                            val lower = name.lowercase(java.util.Locale.ROOT)
                            if (mime == DocumentsContract.Document.MIME_TYPE_DIR) {
                                if (depth < MusicDocuments.TREE_DEPTH_AT_MOST)
                                    pending.add(id to depth + 1)
                            } else if (lower.substringAfterLast('.', "") in extensions)
                                audio.add(
                                    ScannedAudio(
                                        uri,
                                        name,
                                        if (cursor.isNull(3)) 0L else cursor.getLong(3),
                                        if (cursor.isNull(4)) 0L else cursor.getLong(4),
                                    )
                                )
                            else if (lower.endsWith(".lrc"))
                                sidecars[lower.removeSuffix(".lrc")] = uri
                            else if (lower in FOLDER_COVER_NAMES) covers[lower] = uri
                        }
                    }
                    val cover =
                        if (audio.isEmpty()) null
                        else
                            FOLDER_COVER_NAMES.firstNotNullOfOrNull { covers[it] }
                                ?.let { readDocumentWithin(it, FOLDER_COVER_AT_MOST) }
                    val covered = ArrayList<ULong>()
                    for ((uri, name, size, modified) in audio) {
                        currentCoroutineContext().ensureActive()
                        try {
                            val track =
                                native.scanDocument(
                                    uri.toString(),
                                    name,
                                    size.coerceAtLeast(0L).toULong(),
                                    modified,
                                )
                            indexed++
                            covered.add(track.id)
                            try {
                                sidecars[
                                        name
                                            .substringBeforeLast('.')
                                            .lowercase(java.util.Locale.ROOT)]
                                    ?.let { sidecar ->
                                        readDocumentWithin(sidecar, 1024 * 1024)?.let {
                                            native.keepSidecarLyrics(track.id, it)
                                        }
                                    }
                            } catch (failure: Exception) {
                                Log.w(
                                    "Resonate",
                                    "Companion artwork or lyrics could not be read",
                                    failure,
                                )
                            }
                        } catch (failure: Exception) {
                            Log.w("Resonate", "Audio document could not be indexed", failure)
                            failed++
                        }
                        mutableStatus.value = scanSummary(indexed, failed)
                    }
                    if (cover != null && covered.isNotEmpty()) {
                        try {
                            native.keepFolderCover(covered, cover)
                        } catch (failure: Exception) {
                            Log.w("Resonate", "Folder artwork could not be kept", failure)
                        }
                    }
                }
            }
            refresh()
            features.refreshArtwork()
            features.refreshFolders()
            features.enrichment.begin()
            mutableStatus.value = scanSummary(indexed, failed)
            if (failed == 0) features.forgetMissing(afterScan = true)
        } catch (cancelled: CancellationException) {
            throw cancelled
        } catch (failure: Exception) {
            report(failure)
        } finally {
            mutableScanning.value = false
        }
    }

    private fun readDocumentWithin(uri: Uri, limit: Int): ByteArray? =
        try {
            context.contentResolver.openInputStream(uri)?.use { stream ->
                stream.readNBytes(limit + 1).takeIf { it.size <= limit }
            }
        } catch (failure: Exception) {
            Log.w("Resonate", "Companion document could not be opened", failure)
            null
        }

    fun rescan() {
        if (features.downloadsEnabled()) scan(MusicDocuments.downloads)
        context.contentResolver.persistedUriPermissions
            .filter { it.isReadPermission }
            .forEach { scan(it.uri) }
    }

    fun dismissStatus() {
        mutableStatus.value = null
    }

    @Synchronized
    fun close() {
        started = false
        features.stop()
        documentScope?.cancel()
        documentScope = null
        playbackJob?.cancel()
        engine = CompletableDeferred()
    }

    private fun scanSummary(indexed: Int, failed: Int): String =
        "Read $indexed ${if (indexed == 1) "song" else "songs"}${if (failed > 0) "; $failed unreadable" else ""}"

    internal fun report(failure: Exception) {
        Log.e("Resonate", "Player operation failed", failure)
        mutableStatus.value = failure.message ?: "The player could not complete that action"
    }

    companion object {
        private const val POLL_EVERY_MS = 250L
        private const val SAVE_EVERY_TICKS = 20
        private const val PAGE_ROWS = 500u
        private const val FOLDER_COVER_AT_MOST = 8 * 1024 * 1024
        private val FOLDER_COVER_NAMES =
            listOf(
                "cover.jpg",
                "cover.png",
                "folder.jpg",
                "folder.png",
                "front.jpg",
                "front.png",
            )
        private val extensions by lazy { audioExtensions().toSet() }
    }
}

data class ScannedAudio(
    val uri: Uri,
    val name: String,
    val size: Long,
    val modified: Long,
)
