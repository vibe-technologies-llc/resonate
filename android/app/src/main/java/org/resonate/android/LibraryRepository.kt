package org.resonate.android

import android.app.Application
import android.content.Context
import android.net.Uri
import android.provider.DocumentsContract
import android.util.Log
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CompletableDeferred
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
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import org.resonate.android.nativecore.AndroidPlayer
import org.resonate.android.nativecore.Documents
import org.resonate.android.nativecore.PlaybackPreferences
import org.resonate.android.nativecore.Snapshot
import org.resonate.android.nativecore.Track
import org.resonate.android.nativecore.TrackFilter
import org.resonate.android.nativecore.defaultTrackFilter

class ResonateApplication : Application() {
    val library by lazy { LibraryRepository(this) }
}

class LibraryRepository(private val context: Context) {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private var engine = CompletableDeferred<AndroidPlayer>()
    private val actions = Channel<(AndroidPlayer) -> Unit>(Channel.UNLIMITED)
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
    val tracks = mutableTracks.asStateFlow()
    val queue = mutableQueue.asStateFlow()
    val snapshot = mutableSnapshot.asStateFlow()
    val status = mutableStatus.asStateFlow()
    val scanning = mutableScanning.asStateFlow()
    val preferences = mutablePreferences.asStateFlow()
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

    private var playbackJob: Job? = null
    private var documentScope: CoroutineScope? = null
    private var search = ""
    private val queryRequest = java.util.concurrent.atomic.AtomicLong()
    val filter = MutableStateFlow<TrackFilter?>(null)
    val features = LibraryFeatures(context, this)

    internal suspend fun native() = engine.await()

    internal suspend fun refresh() {
        val native = engine.await()
        access.withLock {
            val found = ArrayList<Track>()
            do {
                val page = native.filteredTracks(defaultTrackFilter(), found.size.toUInt(), 500u)
                found.addAll(page)
            } while (page.size == 500)
            mutableAllTracks.value = found
        }
        reload()
    }

    internal fun notice(message: String) {
        mutableStatus.value = message
    }

    fun setFilter(value: TrackFilter) {
        filter.value = value
        scope.launch { reload() }
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
        val ready = CompletableDeferred<AndroidPlayer>()
        engine = ready
        playbackJob = scope.launch {
            previous?.join()
            var opened: AndroidPlayer? = null
            try {
                val native =
                    AndroidPlayer(
                        context.filesDir.absolutePath,
                        object : Documents {
                            override fun open(uri: String): Int =
                                try {
                                    context.contentResolver
                                        .openFileDescriptor(Uri.parse(uri), "r")
                                        ?.detachFd() ?: -1
                                } catch (failure: Exception) {
                                    Log.w("Resonate", "Document grant could not be opened", failure)
                                    -1
                                }
                        },
                    )
                opened = native
                val preferences = preferencesStore().read()
                native.configurePlayback(preferences)
                mutablePreferences.value = preferences
                features.initialize(native)
                ready.complete(native)
                refresh()
                var saves = 0
                while (isActive) {
                    access.withLock {
                        val state = native.snapshot()
                        features.tick(native, state)
                        mutableSnapshot.value = state
                        state.failure?.let { mutableStatus.value = it }
                        mutableQueue.value = native.queue()
                        if (++saves >= 20) {
                            native.save()
                            saves = 0
                        }
                    }
                    delay(250)
                }
            } catch (cancelled: CancellationException) {
                ready.cancel(cancelled)
                throw cancelled
            } catch (failure: Exception) {
                ready.completeExceptionally(failure)
                report(failure)
            } catch (failure: LinkageError) {
                ready.completeExceptionally(failure)
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
                }
            }
        }
    }

    fun request(action: (AndroidPlayer) -> Unit) {
        actions.trySend(action)
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
            scope.launch { reload() }
        }
    }

    fun updatePreferences(change: (PlaybackPreferences) -> PlaybackPreferences) {
        request { native ->
            val next = change(native.playbackPreferences())
            native.configurePlayback(next)
            mutablePreferences.value = next
            preferencesStore().write(next)
        }
    }

    private fun preferencesStore() =
        PlaybackPreferencesStore(context.getSharedPreferences("playback", Context.MODE_PRIVATE))

    fun search(text: String) {
        search = text
        scope.launch { reload() }
    }

    private suspend fun reload() {
        val request = queryRequest.incrementAndGet()
        val query = (filter.value ?: defaultTrackFilter()).copy(text = search)
        val native = engine.await()
        access.withLock {
            val found = ArrayList<Track>()
            do {
                val page = native.filteredTracks(query, found.size.toUInt(), 500u)
                found.addAll(page)
            } while (page.size == 500)
            if (request == queryRequest.get()) mutableTracks.value = found
        }
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
                    )
                val audio = ArrayList<Pair<Uri, String>>()
                val sidecars = HashMap<String, Uri>()
                val covers = HashMap<String, Uri>()
                context.contentResolver.query(children, columns, null, null, null)?.use { cursor ->
                    while (cursor.moveToNext()) {
                        currentCoroutineContext().ensureActive()
                        val id = cursor.getString(0)
                        val name = cursor.getString(1) ?: continue
                        val mime = cursor.getString(2)
                        val uri = DocumentsContract.buildDocumentUriUsingTree(tree, id)
                        val lower = name.lowercase(java.util.Locale.ROOT)
                        if (mime == DocumentsContract.Document.MIME_TYPE_DIR) {
                            if (depth < 32) pending.add(id to depth + 1)
                        } else if (lower.substringAfterLast('.', "") in extensions)
                            audio.add(uri to name)
                        else if (lower.endsWith(".lrc")) sidecars[lower.removeSuffix(".lrc")] = uri
                        else if (
                            lower in
                                listOf(
                                    "cover.jpg",
                                    "folder.jpg",
                                    "front.jpg",
                                    "cover.png",
                                    "folder.png",
                                    "front.png",
                                )
                        )
                            covers[lower] = uri
                    }
                }
                val cover =
                    listOf(
                            "cover.jpg",
                            "cover.png",
                            "folder.jpg",
                            "folder.png",
                            "front.jpg",
                            "front.png",
                        )
                        .firstNotNullOfOrNull { covers[it] }
                        ?.let { readDocumentWithin(it, 8 * 1024 * 1024) }
                for ((uri, name) in audio) {
                    currentCoroutineContext().ensureActive()
                    try {
                        val track = native.indexDocument(uri.toString(), name)
                        indexed++
                        try {
                            if (cover != null) native.setArtwork(track.id, cover)
                            sidecars[name.substringBeforeLast('.').lowercase(java.util.Locale.ROOT)]
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
            }
            refresh()
            features.refreshArtwork()
            features.refreshFolders()
            mutableStatus.value = scanSummary(indexed, failed)
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
    }

    private fun scanSummary(indexed: Int, failed: Int): String =
        "Read $indexed ${if (indexed == 1) "song" else "songs"}${if (failed > 0) "; $failed unreadable" else ""}"

    internal fun report(failure: Exception) {
        Log.e("Resonate", "Player operation failed", failure)
        mutableStatus.value = failure.message ?: "The player could not complete that action"
    }

    companion object {
        private val extensions =
            setOf(
                "aac",
                "aif",
                "aiff",
                "ape",
                "caf",
                "dff",
                "dsf",
                "flac",
                "m4a",
                "m4b",
                "mka",
                "mp3",
                "mp4",
                "oga",
                "ogg",
                "opus",
                "rf64",
                "w64",
                "wav",
                "wave",
                "wv",
            )
    }
}
