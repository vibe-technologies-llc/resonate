package org.resonate.android

import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import android.net.Uri
import android.provider.DocumentsContract
import android.util.LruCache
import java.util.concurrent.atomic.AtomicLong
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancelChildren
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.delay
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import org.resonate.android.nativecore.AnalysisReport
import org.resonate.android.nativecore.AndroidPlayer
import org.resonate.android.nativecore.ArtistInformation
import org.resonate.android.nativecore.AutoEqDevice
import org.resonate.android.nativecore.DownloadAccount
import org.resonate.android.nativecore.HandedOff
import org.resonate.android.nativecore.Inspector
import org.resonate.android.nativecore.LibraryStatistics
import org.resonate.android.nativecore.LyricFrame
import org.resonate.android.nativecore.LyricSheet
import org.resonate.android.nativecore.MusicPlaylist
import org.resonate.android.nativecore.ReceivedSong
import org.resonate.android.nativecore.ScrobbleAccount
import org.resonate.android.nativecore.ScrobbleService
import org.resonate.android.nativecore.SearchAlbum
import org.resonate.android.nativecore.SearchArtist
import org.resonate.android.nativecore.SearchRelease
import org.resonate.android.nativecore.SearchSong
import org.resonate.android.nativecore.Snapshot
import org.resonate.android.nativecore.SyncConfig
import org.resonate.android.nativecore.SyncStanding
import org.resonate.android.nativecore.SyncStatus
import org.resonate.android.nativecore.TidalAuthorization
import org.resonate.android.nativecore.Track
import org.resonate.android.nativecore.syncNewCode

data class AudioOutput(
    val id: Int,
    val name: String,
    val rates: List<Int>,
    val channels: List<Int>,
    val encodings: List<Int>,
)

data class MusicFolder(val uri: Uri, val name: String, val readable: Boolean, val writable: Boolean)

data class PlaylistSync(val playlist: ULong, val missing: UInt, val kept: Boolean)

private fun mimeOf(extension: String) =
    when (extension) {
        "flac" -> "audio/flac"
        "mp3" -> "audio/mpeg"
        "m4a" -> "audio/mp4"
        "ogg",
        "opus" -> "audio/ogg"
        else -> "application/octet-stream"
    }

class LibraryFeatures(private val context: Context, private val library: LibraryRepository) {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val accounts = ProviderAccountStore(context)
    private val settings = context.getSharedPreferences("library-features", Context.MODE_PRIVATE)
    val tidalAuthorization = MutableStateFlow<TidalAuthorization?>(null)
    val tidalBusy = MutableStateFlow(false)
    private val artistRequest = AtomicLong()
    private val analysisRequest = AtomicLong()
    val corrections = MutableStateFlow<List<AutoEqDevice>>(emptyList())
    val correctionBusy = MutableStateFlow(false)
    val scrobbleAccount = MutableStateFlow<ScrobbleAccount?>(null)
    val scrobbleStatus = MutableStateFlow<String?>(null)
    private var submitting: Job? = null
    val outputs = MutableStateFlow<List<AudioOutput>>(emptyList())
    val selectedOutput = MutableStateFlow(0)
    val actualOutput = MutableStateFlow(0)
    val providerAccount = MutableStateFlow<DownloadAccount?>(null)
    val downloading = MutableStateFlow(false)
    val downloadedBytes = MutableStateFlow(0uL)
    val downloadStatus = MutableStateFlow<String?>(null)
    val online = MutableStateFlow(settings.getBoolean("online", true))
    val folders = MutableStateFlow<List<MusicFolder>>(emptyList())
    val statistics = MutableStateFlow<LibraryStatistics?>(null)
    val lyrics = MutableStateFlow<LyricSheet?>(null)
    val lyricFrame = MutableStateFlow<LyricFrame?>(null)
    val lyricsLoading = MutableStateFlow(false)
    val recognitionKey = MutableStateFlow("")
    val recognition =
        MutableStateFlow<List<org.resonate.android.nativecore.RecognitionCandidate>>(emptyList())
    val recognitionBusy = MutableStateFlow(false)
    private val recognitionRequest = AtomicLong()
    val artistAlbumsNext = MutableStateFlow<UInt?>(null)
    val analysis = MutableStateFlow<AnalysisReport?>(null)
    val analysisBusy = MutableStateFlow(false)
    val analysisProgress = MutableStateFlow<Float?>(null)
    val inspector = MutableStateFlow<Inspector?>(null)
    val artist = MutableStateFlow<ArtistInformation?>(null)
    val playlists = MutableStateFlow<List<MusicPlaylist>>(emptyList())
    val playlistRows = MutableStateFlow<List<Track>>(emptyList())
    val openPlaylist = MutableStateFlow<ULong?>(null)
    val onlineAlbums = MutableStateFlow<List<SearchAlbum>>(emptyList())
    val onlineArtists = MutableStateFlow<List<SearchArtist>>(emptyList())
    val onlineReleases = MutableStateFlow<List<SearchRelease>>(emptyList())
    val onlineSongs = MutableStateFlow<List<SearchSong>>(emptyList())
    val onlineCollection = MutableStateFlow<org.resonate.android.nativecore.OnlineCollection?>(null)
    val wanted = MutableStateFlow<List<org.resonate.android.nativecore.WantedSong>>(emptyList())
    val searching = MutableStateFlow(false)
    val enriching = MutableStateFlow(false)
    val samples = MutableStateFlow<List<Float>>(emptyList())
    val artworkGeneration = MutableStateFlow(0)
    private val lyricRequest = AtomicLong()
    private val searchRequest = AtomicLong()
    private var visualization: Job? = null
    private var downloadReady = CompletableDeferred<Unit>()
    val syncConfig = MutableStateFlow<SyncConfig?>(null)
    val syncStatus = MutableStateFlow<SyncStatus?>(null)
    val syncHandoffs = MutableStateFlow<List<HandedOff>>(emptyList())
    val syncFolder = MutableStateFlow(settings.getString("sync-folder", null))
    val syncNotice = MutableStateFlow<String?>(null)
    val playlistSync = MutableStateFlow<PlaylistSync?>(null)
    val transferring = MutableStateFlow(false)
    val transferServiceRunning = MutableStateFlow(false)
    private val syncAccess = Mutex()
    private var syncPolling: Job? = null
    private var network: ConnectivityManager.NetworkCallback? = null
    @Volatile private var unmetered = false
    @Volatile private var foreground = true
    @Volatile private var landingRefusedUntil = 0L

    fun downloadServiceReady() {
        downloadReady.complete(Unit)
    }

    private val pictures =
        object : LruCache<String, Bitmap>(32 * 1024 * 1024) {
            override fun sizeOf(key: String, value: Bitmap) = value.allocationByteCount
        }

    fun initialize(native: AndroidPlayer) {
        providerAccount.value =
            try {
                accounts.read()
            } catch (failure: Exception) {
                library.report(failure)
                DownloadAccount(
                    org.resonate.android.nativecore.MusicProvider.MONOCHROME,
                    "",
                    "",
                    "",
                    "",
                    "",
                    "",
                )
            }
        scrobbleAccount.value =
            try {
                accounts.readScrobbling()
            } catch (failure: Exception) {
                library.report(failure)
                ScrobbleAccount("", "", "", "")
            }
        recognitionKey.value =
            try {
                accounts.readRecognitionKey()
            } catch (failure: Exception) {
                library.report(failure)
                ""
            }
        submitting?.cancel()
        submitting = scope.launch {
            var lastPlaying: ULong? = null
            while (isActive) {
                delay(30000)
                val account = scrobbleAccount.value ?: continue
                if (
                    !online.value ||
                        account.listenbrainzToken.isEmpty() && account.lastfmSession.isEmpty()
                )
                    continue
                val state = library.snapshot.value
                val playing =
                    state?.current?.id?.takeIf {
                        state.transport == org.resonate.android.nativecore.Transport.PLAYING
                    }
                try {
                    val count =
                        library
                            .native()
                            .submitScrobbles(account, playing?.takeIf { it != lastPlaying })
                    lastPlaying = playing
                    if (count > 0uL) scrobbleStatus.value = "Submitted $count listens"
                } catch (failure: Exception) {
                    scrobbleStatus.value = failure.message
                }
            }
        }
        native.configureOnline(online.value)
        if (!downloading.value)
            settings.getString("unfinished-download", null)?.let { value ->
                try {
                    DocumentsContract.deleteDocument(context.contentResolver, Uri.parse(value))
                    settings.edit().remove("unfinished-download").commit()
                } catch (failure: Exception) {
                    library.report(failure)
                }
            }
        settings.getString("unfinished-sync-landing", null)?.let { value ->
            try {
                DocumentsContract.deleteDocument(context.contentResolver, Uri.parse(value))
                settings.edit().remove("unfinished-sync-landing").commit()
            } catch (failure: Exception) {
                library.report(failure)
            }
        }
        refreshFolders()
        syncConfig.value =
            try {
                accounts.readSync()
            } catch (failure: Exception) {
                library.report(failure)
                SyncConfig(true, "", "", "", false, false, false, false)
            }
        watchNetwork()
        syncPolling?.cancel()
        syncPolling = scope.launch { pollSync() }
        work { applySync(it) }
    }

    fun setOnline(enabled: Boolean) {
        work { native ->
            native.configureOnline(enabled)
            online.value = enabled
            settings.edit().putBoolean("online", enabled).commit()
            applySync(native)
        }
    }

    fun saveSync(config: SyncConfig) = work { native ->
        accounts.writeSync(config)
        syncConfig.value = config
        applySync(native)
    }

    fun joinSync(code: String) {
        val config = syncConfig.value ?: return
        saveSync(config.copy(key = code.trim()))
    }

    fun newSyncGroup() = work { native ->
        val config = syncConfig.value ?: return@work
        val fresh = config.copy(key = syncNewCode())
        accounts.writeSync(fresh)
        syncConfig.value = fresh
        applySync(native)
    }

    fun chooseSyncFolder(folder: MusicFolder) {
        syncFolder.value = folder.uri.toString()
        scope.launch { settings.edit().putString("sync-folder", folder.uri.toString()).commit() }
    }

    fun foreground(visible: Boolean) {
        foreground = visible
    }

    fun loadPlaylistSync(id: ULong) = work { native ->
        playlistSync.value =
            PlaylistSync(id, native.syncMissingFrom(id), native.syncKeepsSongs(id))
    }

    fun fetchMissing(id: ULong, device: String) = work { native ->
        val asked = native.syncFetchMissing(id, device)
        library.notice(
            if (asked > 0u) "Asking $device for $asked ${if (asked == 1u) "song" else "songs"}"
            else "$device has nothing this playlist lacks"
        )
    }

    fun keepSongs(id: ULong, keep: Boolean) = work { native ->
        native.syncKeepSongs(id, keep)
        playlistSync.value =
            PlaylistSync(id, native.syncMissingFrom(id), native.syncKeepsSongs(id))
    }

    fun continueFrom(handoff: HandedOff, play: () -> Unit) = work { native ->
        val loaded = native.syncContinueFrom(handoff.device)
        library.notice(
            "Continuing ${if (loaded == 1u) "1 song" else "$loaded songs"} from ${handoff.device}"
        )
        if (handoff.playing) withContext(Dispatchers.Main) { play() }
    }

    private suspend fun applySync(native: AndroidPlayer) {
        syncAccess.withLock { syncWith(native) }
    }

    private fun syncWith(native: AndroidPlayer) {
        val config = syncConfig.value ?: return
        try {
            val status =
                if (config.enabled && online.value) native.syncStart(config)
                else {
                    native.syncStop()
                    native.syncStatus()
                }
            if (status.code.isNotEmpty() && status.code != config.key) {
                val kept = config.copy(key = status.code)
                accounts.writeSync(kept)
                syncConfig.value = kept
            }
            native.syncNetwork(unmetered)
            syncStatus.value = status
        } catch (failure: Exception) {
            syncStatus.value = native.syncStatus()
            throw failure
        }
    }

    private fun watchNetwork() {
        if (network != null) return
        val callback =
            object : ConnectivityManager.NetworkCallback() {
                override fun onCapabilitiesChanged(
                    network: Network,
                    networkCapabilities: NetworkCapabilities,
                ) {
                    networkChanged(
                        networkCapabilities.hasCapability(
                            NetworkCapabilities.NET_CAPABILITY_NOT_METERED
                        )
                    )
                }

                override fun onLost(network: Network) {
                    networkChanged(false)
                }
            }
        try {
            context
                .getSystemService(ConnectivityManager::class.java)
                .registerDefaultNetworkCallback(callback)
            network = callback
        } catch (failure: Exception) {
            library.report(failure)
        }
    }

    private fun networkChanged(free: Boolean) {
        if (free == unmetered) return
        unmetered = free
        work { it.syncNetwork(free) }
    }

    private suspend fun pollSync() {
        var arrivals = 0uL
        while (currentCoroutineContext().isActive) {
            delay(if (foreground || transferring.value) 2000L else 30000L)
            try {
                val native = library.native()
                val status = native.syncStatus()
                syncStatus.value = status
                val waiting = native.syncReceived()
                transferring.value = status.moving > 0u || waiting.isNotEmpty()
                if (transferring.value) holdTransfers()
                if (waiting.isNotEmpty()) landReceived(native, waiting)
                syncHandoffs.value =
                    if (status.standing == SyncStanding.OFF) emptyList() else native.syncHandoffs()
                if (arrivals != status.arrivals) {
                    arrivals = status.arrivals
                    library.refresh()
                    playlists.value = native.playlists()
                    openPlaylist.value?.let { id ->
                        if (playlists.value.any { it.id == id }) reloadPlaylist(native, id)
                    }
                }
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (failure: Exception) {
                library.report(failure)
            }
        }
    }

    private fun holdTransfers() {
        if (transferServiceRunning.value || downloading.value) return
        try {
            context.startForegroundService(Intent(context, DownloadService::class.java))
        } catch (failure: Exception) {
            android.util.Log.w(
                "Resonate",
                "Device sync continues without a foreground service",
                failure,
            )
        }
    }

    private suspend fun landReceived(native: AndroidPlayer, waiting: List<ReceivedSong>) {
        if (System.currentTimeMillis() < landingRefusedUntil) return
        val destinations = folders.value.filter { it.readable && it.writable }
        val folder =
            destinations.find { it.uri.toString() == syncFolder.value }
                ?: destinations.firstOrNull()
        if (folder == null) {
            syncNotice.value = "Add a music folder with write access to keep songs from your devices"
            return
        }
        var landed = 0
        for (received in waiting) {
            currentCoroutineContext().ensureActive()
            var created: Uri? = null
            try {
                val parent =
                    DocumentsContract.buildDocumentUriUsingTree(
                        folder.uri,
                        DocumentsContract.getTreeDocumentId(folder.uri),
                    )
                val uri =
                    requireNotNull(
                        DocumentsContract.createDocument(
                            context.contentResolver,
                            parent,
                            mimeOf(received.extension),
                            received.name,
                        )
                    )
                created = uri
                check(
                    settings.edit().putString("unfinished-sync-landing", uri.toString()).commit()
                ) {
                    "The song from another device could not be recorded"
                }
                val descriptor =
                    requireNotNull(context.contentResolver.openFileDescriptor(uri, "w")).detachFd()
                native.syncReceive(received.id, descriptor, uri.toString())
                created = null
                settings.edit().remove("unfinished-sync-landing").commit()
                landed++
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (failure: Exception) {
                landingRefusedUntil = System.currentTimeMillis() + 60000L
                syncNotice.value = failure.message ?: "A song from another device could not be kept"
                library.report(failure)
                break
            } finally {
                created?.let { uri ->
                    try {
                        DocumentsContract.deleteDocument(context.contentResolver, uri)
                        settings.edit().remove("unfinished-sync-landing").commit()
                    } catch (failure: Exception) {
                        library.report(failure)
                    }
                }
            }
        }
        if (landed > 0) {
            syncNotice.value = null
            library.refresh()
            refreshWanted(native)
            refreshArtwork()
            library.notice("Kept $landed ${if (landed == 1) "song" else "songs"} from your devices")
        }
    }

    fun refreshFolders() {
        scope.launch {
            val granted =
                context.contentResolver.persistedUriPermissions
                    .filter { it.isReadPermission }
                    .map { it.uri.toString() }
                    .toSet()
            val known = settings.getStringSet("folders", emptySet()).orEmpty() + granted
            val found =
                known
                    .map { value ->
                        val uri = Uri.parse(value)
                        var name =
                            settings.getString("folder-name:$value", null)
                                ?: uri.lastPathSegment.orEmpty().substringAfter(':')
                        var readable = false
                        if (value in granted) {
                            try {
                                val document =
                                    DocumentsContract.buildDocumentUriUsingTree(
                                        uri,
                                        DocumentsContract.getTreeDocumentId(uri),
                                    )
                                context.contentResolver
                                    .query(
                                        document,
                                        arrayOf(DocumentsContract.Document.COLUMN_DISPLAY_NAME),
                                        null,
                                        null,
                                        null,
                                    )
                                    ?.use { cursor ->
                                        if (cursor.moveToFirst()) {
                                            name = cursor.getString(0) ?: name
                                            readable = true
                                        }
                                    }
                            } catch (failure: SecurityException) {
                                readable = false
                            }
                        }
                        MusicFolder(
                            uri,
                            name.ifBlank { "Music folder" },
                            readable,
                            context.contentResolver.persistedUriPermissions.any {
                                it.uri == uri && it.isWritePermission
                            },
                        )
                    }
                    .sortedBy { it.name.lowercase() }
            val edit = settings.edit().putStringSet("folders", known)
            found.forEach { edit.putString("folder-name:${it.uri}", it.name) }
            edit.commit()
            folders.value = found
        }
    }

    fun removeFolder(folder: MusicFolder) {
        scope.launch {
            try {
                val permission =
                    context.contentResolver.persistedUriPermissions.find { it.uri == folder.uri }
                if (permission != null)
                    context.contentResolver.releasePersistableUriPermission(
                        folder.uri,
                        (if (permission.isReadPermission) Intent.FLAG_GRANT_READ_URI_PERMISSION
                        else 0) or
                            (if (permission.isWritePermission)
                                Intent.FLAG_GRANT_WRITE_URI_PERMISSION
                            else 0),
                    )
                settings
                    .edit()
                    .putStringSet(
                        "folders",
                        settings.getStringSet("folders", emptySet()).orEmpty() -
                            folder.uri.toString(),
                    )
                    .remove("folder-name:${folder.uri}")
                    .commit()
                refreshFolders()
            } catch (failure: Exception) {
                library.report(failure)
            }
        }
    }

    fun refreshArtwork() {
        pictures.evictAll()
        artworkGeneration.value++
    }

    suspend fun artwork(track: Track): Bitmap? =
        withContext(Dispatchers.IO) {
            val key = "${track.id}:${track.artRevision}:${artworkGeneration.value}"
            pictures.get(key)?.let {
                return@withContext it
            }
            try {
                val bytes = library.native().artwork(track.id) ?: return@withContext null
                val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
                BitmapFactory.decodeByteArray(bytes, 0, bytes.size, bounds)
                if (
                    bounds.outWidth <= 0 ||
                        bounds.outHeight <= 0 ||
                        bounds.outWidth > 32768 ||
                        bounds.outHeight > 32768
                )
                    return@withContext null
                val options = BitmapFactory.Options().apply { inSampleSize = 1 }
                while (
                    bounds.outWidth / options.inSampleSize > 1024 ||
                        bounds.outHeight / options.inSampleSize > 1024
                ) options.inSampleSize *= 2
                BitmapFactory.decodeByteArray(bytes, 0, bytes.size, options)?.also {
                    pictures.put(key, it)
                }
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (failure: Exception) {
                null
            }
        }

    suspend fun mediaArtwork(track: Track): ByteArray? =
        withContext(Dispatchers.IO) {
            val source = artwork(track) ?: return@withContext null
            for (edge in listOf(384, 192)) {
                val scale = minOf(1f, edge.toFloat() / maxOf(source.width, source.height))
                val thumbnail =
                    Bitmap.createScaledBitmap(
                        source,
                        (source.width * scale).toInt().coerceAtLeast(1),
                        (source.height * scale).toInt().coerceAtLeast(1),
                        true,
                    )
                val encoded =
                    try {
                        java.io.ByteArrayOutputStream().use { output ->
                            if (thumbnail.compress(Bitmap.CompressFormat.JPEG, 85, output))
                                output.toByteArray()
                            else null
                        }
                    } finally {
                        if (thumbnail !== source) thumbnail.recycle()
                    }
                if (encoded != null && encoded.size <= 128 * 1024) return@withContext encoded
            }
            null
        }

    fun loadPlaylists() = work { native -> playlists.value = native.playlists() }

    fun selectPlaylist(id: ULong?) {
        openPlaylist.value = id
        playlistRows.value = emptyList()
        if (id != null) work { native -> reloadPlaylist(native, id) }
    }

    fun createPlaylist(name: String, track: Track? = null) = work { native ->
        val id = native.createPlaylist(name)
        if (track != null) native.addToPlaylist(id, listOf(track.id))
        playlists.value = native.playlists()
        if (openPlaylist.value == id) reloadPlaylist(native, id)
    }

    fun addToPlaylist(id: ULong, track: Track) = addToPlaylist(id, listOf(track))

    fun addToPlaylist(id: ULong, tracks: List<Track>) = work { native ->
        native.addToPlaylist(id, tracks.map { it.id })
        playlists.value = native.playlists()
        if (openPlaylist.value == id) reloadPlaylist(native, id)
    }

    fun editPlaylist(action: (AndroidPlayer) -> Unit) = work { native ->
        action(native)
        playlists.value = native.playlists()
        openPlaylist.value?.let { id ->
            if (playlists.value.any { it.id == id }) reloadPlaylist(native, id)
            else selectPlaylist(null)
        }
    }

    private fun reloadPlaylist(native: AndroidPlayer, id: ULong) {
        val rows = ArrayList<Track>()
        do {
            val page = native.playlistTracks(id, rows.size.toUInt(), 500u)
            rows.addAll(page)
        } while (page.size == 500)
        if (openPlaylist.value == id) playlistRows.value = rows
        playlists.value = native.playlists()
    }

    fun loadStatistics(days: UInt = 0u) = work { native ->
        statistics.value = native.statistics(days)
    }

    fun loadInspector(id: ULong) {
        inspector.value = null
        artistRequest.incrementAndGet()
        artist.value = null
        work { native -> inspector.value = native.inspector(id) }
    }

    fun loadArtist(id: ULong) {
        val request = artistRequest.incrementAndGet()
        artist.value = null
        work { native ->
            val found = native.artistInformation(id)
            if (request == artistRequest.get()) artist.value = found
        }
    }

    fun onlineArtist(id: String) {
        val request = artistRequest.incrementAndGet()
        artist.value = null
        work { native ->
            val found = native.onlineArtist(id)
            if (request == artistRequest.get()) artist.value = found
        }
    }

    fun loadLyrics(id: ULong, refresh: Boolean = false) {
        val request = lyricRequest.incrementAndGet()
        lyrics.value = null
        lyricFrame.value = null
        lyricsLoading.value = true
        work { native ->
            try {
                val sheet = native.lyrics(id, refresh)
                if (request == lyricRequest.get()) lyrics.value = sheet
            } finally {
                if (request == lyricRequest.get()) lyricsLoading.value = false
            }
        }
    }

    fun tick(native: AndroidPlayer, snapshot: Snapshot) {
        lyrics.value?.let { sheet ->
            if (snapshot.current?.id == sheet.trackId)
                lyricFrame.value = native.lyricFrame(sheet.trackId, snapshot.positionMs)
        }
        actualOutput.value = native.outputDevice()
        if (downloading.value) downloadedBytes.value = native.downloadBytes()
        if (analysisBusy.value) analysisProgress.value = native.analysisProgress()
    }

    fun saveRecognitionKey(value: String) = work {
        accounts.writeRecognitionKey(value)
        recognitionKey.value = value
    }

    fun recognise(id: ULong) {
        val request = recognitionRequest.incrementAndGet()
        recognition.value = emptyList()
        recognitionBusy.value = true
        work { native ->
            try {
                val found = native.recogniseTrack(id, recognitionKey.value)
                if (request == recognitionRequest.get()) {
                    recognition.value = found
                    if (found.isEmpty())
                        library.notice("No recording could be identified from this audio")
                }
            } finally {
                if (request == recognitionRequest.get()) recognitionBusy.value = false
            }
        }
    }

    fun analyse(id: ULong) {
        recognitionRequest.incrementAndGet()
        recognition.value = emptyList()
        recognitionBusy.value = false
        val request = analysisRequest.incrementAndGet()
        analysis.value = null
        analysisBusy.value = true
        work { native ->
            try {
                val report = native.analyseTrack(id)
                if (request == analysisRequest.get()) analysis.value = report
            } catch (failure: Exception) {
                if (request == analysisRequest.get()) throw failure
            } finally {
                if (request == analysisRequest.get()) analysisBusy.value = false
            }
        }
    }

    fun cancelAnalysis() {
        val request = analysisRequest.incrementAndGet()
        analysisBusy.value = false
        work { if (request == analysisRequest.get()) it.cancelAnalysis() }
    }

    fun enrich(tracks: List<Track>) {
        if (enriching.value) return
        enriching.value = true
        work { native ->
            try {
                var failures = 0
                tracks.forEachIndexed { index, track ->
                    currentCoroutineContext().ensureActive()
                    library.notice("Enriching ${index + 1}/${tracks.size}: ${track.title}")
                    try {
                        native.enrichTrack(track.id)
                    } catch (failure: Exception) {
                        failures++
                        library.report(failure)
                    }
                }
                library.refresh()
                refreshArtwork()
                library.notice(
                    "Enrichment finished${if (failures > 0) "; $failures lookups could not complete" else ""}"
                )
            } finally {
                enriching.value = false
            }
        }
    }

    fun searchCorrections(text: String) {
        correctionBusy.value = true
        work { native ->
            try {
                corrections.value = native.searchAutoEq(text)
            } finally {
                correctionBusy.value = false
            }
        }
    }

    fun applyCorrection(device: AutoEqDevice) {
        correctionBusy.value = true
        work { native ->
            try {
                val profile = native.autoEqProfile(device.id) ?: return@work
                library.updatePreferences {
                    it.copy(
                        equaliserEnabled = true,
                        profileName = profile.name,
                        profileText = profile.text,
                    )
                }
                library.notice("Applied ${profile.name}")
            } finally {
                correctionBusy.value = false
            }
        }
    }

    fun saveScrobbling(account: ScrobbleAccount, includeHistory: Boolean) = work { native ->
        val previous = scrobbleAccount.value
        if (
            account.listenbrainzToken.isNotEmpty() &&
                (previous?.listenbrainzToken != account.listenbrainzToken || includeHistory)
        )
            native.startScrobbling(ScrobbleService.LISTEN_BRAINZ, includeHistory)
        if (
            account.lastfmSession.isNotEmpty() &&
                (previous?.lastfmSession != account.lastfmSession || includeHistory)
        )
            native.startScrobbling(ScrobbleService.LAST_FM, includeHistory)
        accounts.writeScrobbling(account)
        scrobbleAccount.value = account
        scrobbleStatus.value = "Scrobbling accounts saved"
    }

    fun sendFavourites() = work { native ->
        val account = scrobbleAccount.value ?: return@work
        val count = native.sendFavourites(account)
        scrobbleStatus.value = "Sent $count favourite updates"
    }

    fun validateScrobbling(account: ScrobbleAccount) = work { native ->
        scrobbleStatus.value = native.validateScrobbling(account).joinToString(" · ")
    }

    fun signInLastFm(account: ScrobbleAccount, user: String, password: String) = work { native ->
        val session = native.lastfmSignIn(account.lastfmKey, account.lastfmSecret, user, password)
        val signedIn = account.copy(lastfmSession = session)
        accounts.writeScrobbling(signedIn)
        native.startScrobbling(ScrobbleService.LAST_FM, false)
        scrobbleAccount.value = signedIn
        scrobbleStatus.value = "Signed in to Last.fm"
    }

    fun refreshOutputs() {
        scope.launch {
            val manager = context.getSystemService(android.media.AudioManager::class.java)
            outputs.value =
                manager.getDevices(android.media.AudioManager.GET_DEVICES_OUTPUTS).map { device ->
                    AudioOutput(
                        device.id,
                        device.productName.toString(),
                        device.sampleRates.toList(),
                        device.channelCounts.toList(),
                        device.encodings.toList(),
                    )
                }
        }
    }

    fun selectOutput(id: Int) {
        library.request { native ->
            native.selectOutput(id)
            selectedOutput.value = id
        }
    }

    fun authorizeTidal(account: DownloadAccount) {
        tidalBusy.value = true
        work { native ->
            try {
                tidalAuthorization.value =
                    native.authorizeTidal(account.clientId, account.clientSecret)
                val token = native.finishTidalAuthorization() ?: return@work
                val updated = account.copy(refreshToken = token)
                accounts.write(updated)
                providerAccount.value = updated
                tidalAuthorization.value = null
                library.notice("Signed in to TIDAL")
            } finally {
                tidalBusy.value = false
            }
        }
    }

    fun saveProviderAccount(account: DownloadAccount) = work {
        accounts.write(account)
        providerAccount.value = account
        library.notice("Download provider saved")
    }

    fun download(song: SearchSong, folder: MusicFolder) {
        if (downloading.value) return
        val account = providerAccount.value ?: return
        downloading.value = true
        downloadReady = CompletableDeferred()
        try {
            context.startForegroundService(Intent(context, DownloadService::class.java))
        } catch (failure: Exception) {
            downloading.value = false
            library.report(failure)
            return
        }
        downloadedBytes.value = 0uL
        downloadStatus.value = "Finding ${song.title}…"
        work { native ->
            var created: Uri? = null
            try {
                downloadReady.await()
                val offer =
                    native.findDownload(song, account)
                        ?: run {
                            downloadStatus.value =
                                "This provider could not find a matching recording"
                            return@work
                        }
                native.refreshedTidalToken()?.let { token ->
                    val renewed = account.copy(refreshToken = token)
                    accounts.write(renewed)
                    providerAccount.value = renewed
                }
                val parent =
                    DocumentsContract.buildDocumentUriUsingTree(
                        folder.uri,
                        DocumentsContract.getTreeDocumentId(folder.uri),
                    )
                val mime = mimeOf(offer.extension)
                val uri =
                    requireNotNull(
                        DocumentsContract.createDocument(
                            context.contentResolver,
                            parent,
                            mime,
                            offer.name,
                        )
                    )
                created = uri
                check(settings.edit().putString("unfinished-download", uri.toString()).commit()) {
                    "The download could not be recorded"
                }
                downloadStatus.value = "Downloading ${song.title}…"
                val descriptor =
                    requireNotNull(context.contentResolver.openFileDescriptor(uri, "w")).detachFd()
                val count = native.downloadOffer(offer.id, descriptor)
                native.indexDownload(uri.toString(), offer.name, song)
                refreshWanted(native)
                created = null
                settings.edit().remove("unfinished-download").commit()
                downloadStatus.value = "Downloaded ${offer.name} · ${count / 1024uL} KiB"
                library.refresh()
                refreshArtwork()
            } catch (failure: Exception) {
                downloadStatus.value = failure.message ?: "Download failed"
                throw failure
            } finally {
                created?.let {
                    try {
                        DocumentsContract.deleteDocument(context.contentResolver, it)
                        settings.edit().remove("unfinished-download").commit()
                    } catch (failure: Exception) {
                        library.report(failure)
                    }
                }
                downloading.value = false
            }
        }
    }

    fun cancelDownload() = work { it.cancelDownload() }

    fun clearCollection() {
        onlineCollection.value = null
    }

    fun openLink(text: String) {
        onlineCollection.value = null
        onlineWork(
            { it.openOnlineLink(text) },
            { collection ->
                onlineSongs.value = collection.songs
                onlineReleases.value = collection.releases
                onlineArtists.value = collection.artists
                onlineCollection.value = collection
                if (
                    collection.songs.isEmpty() &&
                        collection.releases.isEmpty() &&
                        collection.artists.isEmpty()
                )
                    library.notice("No music could be read from this link")
            },
        )
    }

    fun saveWanted(songs: List<SearchSong>) = work { native ->
        var count = 0uL
        songs.chunked(500).forEach { count += native.saveWanted(it) }
        refreshWanted(native)
        library.notice("Saved $count recordings for later")
    }

    fun loadWanted() = work { refreshWanted(it) }

    fun removeWanted(id: ULong) = work { native ->
        native.removeWanted(id)
        refreshWanted(native)
    }

    private fun refreshWanted(native: AndroidPlayer) {
        val rows = ArrayList<org.resonate.android.nativecore.WantedSong>()
        do {
            val page = native.wantedSongs(rows.size.toUInt(), 500u)
            rows.addAll(page)
        } while (page.size == 500)
        wanted.value = rows
    }

    fun importPlaylist(name: String, songs: List<SearchSong>) = work { native ->
        val imported = native.importMatchedPlaylist(name, songs)
        playlists.value = native.playlists()
        library.notice(
            "Imported ${imported.matched} local songs; ${imported.missing} still missing"
        )
    }

    fun artistAlbums(id: String, from: UInt = 0u) =
        onlineWork(
            { it.artistAlbums(id, from) },
            { page ->
                onlineAlbums.value =
                    if (from == 0u) page.albums
                    else (onlineAlbums.value + page.albums).distinctBy { it.id }
                artistAlbumsNext.value = page.next
            },
        )

    fun searchAlbums(text: String) =
        onlineWork(
            {
                clearCollection()
                it.searchAlbumsOnline(text)
            },
            {
                onlineAlbums.value = it
                artistAlbumsNext.value = null
            },
        )

    fun searchArtists(text: String) =
        onlineWork(
            {
                clearCollection()
                it.searchArtistsOnline(text)
            },
            { onlineArtists.value = it },
        )

    fun onlineReleases(group: String) =
        onlineWork({ it.onlineReleases(group) }, { onlineReleases.value = it })

    fun releaseSongs(id: String) = onlineWork({ it.releaseSongs(id) }, { onlineSongs.value = it })

    private fun <T> onlineWork(action: (AndroidPlayer) -> T, publish: (T) -> Unit) {
        val request = searchRequest.incrementAndGet()
        searching.value = true
        work { native ->
            try {
                val value = action(native)
                if (request == searchRequest.get()) publish(value)
            } finally {
                if (request == searchRequest.get()) searching.value = false
            }
        }
    }

    fun applyProfileText(name: String, text: String) = work { native ->
        val profile = native.parseEqProfile(text.toByteArray(Charsets.UTF_8), name)
        library.updatePreferences {
            it.copy(equaliserEnabled = true, profileName = profile.name, profileText = profile.text)
        }
    }

    fun searchOnline(text: String) {
        clearCollection()
        val request = searchRequest.incrementAndGet()
        searching.value = true
        work { native ->
            try {
                val result = native.searchOnline(text)
                if (request == searchRequest.get()) onlineSongs.value = result
            } finally {
                if (request == searchRequest.get()) searching.value = false
            }
        }
    }

    fun visualize(enabled: Boolean) {
        visualization?.cancel()
        visualization = null
        if (enabled)
            visualization = scope.launch {
                val native = library.native()
                try {
                    while (isActive) {
                        samples.value = native.visualizer(true)
                        delay(33)
                    }
                } finally {
                    try {
                        native.visualizer(false)
                    } catch (failure: Exception) {
                        samples.value = emptyList()
                    }
                }
            }
    }

    fun stop() {
        scope.coroutineContext.cancelChildren()
        submitting = null
        visualization = null
        syncPolling = null
        network?.let { callback ->
            try {
                context
                    .getSystemService(ConnectivityManager::class.java)
                    .unregisterNetworkCallback(callback)
            } catch (failure: Exception) {
                android.util.Log.w("Resonate", "Network watch could not be removed", failure)
            }
        }
        network = null
        unmetered = false
        transferring.value = false
        syncHandoffs.value = emptyList()
        analysisRequest.incrementAndGet()
        recognitionRequest.incrementAndGet()
        recognitionBusy.value = false
        artistRequest.incrementAndGet()
        lyricRequest.incrementAndGet()
        searchRequest.incrementAndGet()
        analysisBusy.value = false
        lyricsLoading.value = false
        searching.value = false
        enriching.value = false
        tidalBusy.value = false
        correctionBusy.value = false
    }

    private fun work(action: suspend (AndroidPlayer) -> Unit) {
        scope.launch {
            try {
                action(library.native())
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (failure: Exception) {
                library.report(failure)
            }
        }
    }
}
