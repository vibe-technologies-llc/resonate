package org.resonate.android

import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.media.AudioDeviceInfo
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import android.net.Uri
import android.net.wifi.WifiManager
import android.os.Build
import android.os.SystemClock
import android.provider.DocumentsContract
import android.provider.Settings
import android.util.LruCache
import java.util.concurrent.atomic.AtomicLong
import org.json.JSONArray
import org.json.JSONException
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineExceptionHandler
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancelChildren
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.delay
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeoutOrNull
import org.resonate.android.nativecore.SavedSearch
import org.resonate.android.nativecore.AnalysisReport
import org.resonate.android.nativecore.ArtistInformation
import org.resonate.android.nativecore.AutoEqDevice
import org.resonate.android.nativecore.DownloadAccount
import org.resonate.android.nativecore.HandedOff
import org.resonate.android.nativecore.LibraryStatistics
import org.resonate.android.nativecore.LyricFrame
import org.resonate.android.nativecore.LyricSheet
import org.resonate.android.nativecore.MissingSongs
import org.resonate.android.nativecore.IdenticalSongs
import org.resonate.android.nativecore.MobilePlayer
import org.resonate.android.nativecore.MusicPlaylist
import org.resonate.android.nativecore.ReceivedSong
import org.resonate.android.nativecore.ScrobbleAccount
import org.resonate.android.nativecore.ScrobbleService
import org.resonate.android.nativecore.SearchAlbum
import org.resonate.android.nativecore.SearchArtist
import org.resonate.android.nativecore.SearchRelease
import org.resonate.android.nativecore.SearchSong
import org.resonate.android.nativecore.OutputStream
import org.resonate.android.nativecore.Snapshot
import org.resonate.android.nativecore.SyncConfig
import org.resonate.android.nativecore.SyncStanding
import org.resonate.android.nativecore.SyncStatus
import org.resonate.android.nativecore.SyncedDevice
import org.resonate.android.nativecore.TidalAuthorization
import org.resonate.android.nativecore.Track
import org.resonate.android.nativecore.syncCodeInLink
import org.resonate.android.nativecore.syncDeviceCalled
import org.resonate.android.nativecore.syncNewCode

data class AudioOutput(
    val id: Int,
    val name: String,
    val kind: String,
    val rates: List<Int>,
    val channels: List<Int>,
    val encodings: List<Int>,
    val route: OutputRoute? = null,
)

internal fun outputKind(type: Int): String =
    when (type) {
        AudioDeviceInfo.TYPE_BUILTIN_SPEAKER,
        AudioDeviceInfo.TYPE_BUILTIN_SPEAKER_SAFE -> "Built-in speaker"
        AudioDeviceInfo.TYPE_BUILTIN_EARPIECE -> "Earpiece"
        AudioDeviceInfo.TYPE_WIRED_HEADPHONES -> "Wired headphones"
        AudioDeviceInfo.TYPE_WIRED_HEADSET -> "Wired headset"
        AudioDeviceInfo.TYPE_BLUETOOTH_A2DP,
        AudioDeviceInfo.TYPE_BLUETOOTH_SCO,
        AudioDeviceInfo.TYPE_BLE_HEADSET,
        AudioDeviceInfo.TYPE_BLE_SPEAKER,
        AudioDeviceInfo.TYPE_BLE_BROADCAST -> "Bluetooth audio"
        AudioDeviceInfo.TYPE_USB_DEVICE,
        AudioDeviceInfo.TYPE_USB_HEADSET,
        AudioDeviceInfo.TYPE_USB_ACCESSORY -> "USB audio"
        AudioDeviceInfo.TYPE_HDMI,
        AudioDeviceInfo.TYPE_HDMI_ARC,
        AudioDeviceInfo.TYPE_HDMI_EARC -> "HDMI audio"
        else -> "Audio device"
    }

data class MusicFolder(val uri: Uri, val name: String, val readable: Boolean, val writable: Boolean)

data class PlaylistSync(val playlist: ULong, val missing: UInt, val kept: Boolean)

data class RefusedLanding(val until: Long, val tries: Int)

private const val LANDING_PAUSE_MS = 60000L
private const val LYRIC_FRAME_EVERY_MS = 33L
private const val LANDING_TRIES_AT_MOST = 3
private const val MISSING_CHECKED_EVERY_MS = 15000L
private const val DEVICE_NAME_SETTING = "device_name"
private const val RECENT_SEARCHES_KEY = "recent-searches"
private const val REMEMBER_SEARCHES_KEY = "remember-searches"
private const val TELL_FINDINGS_KEY = "tell-findings"
private const val RECENT_SEARCHES_KEPT = 10

class LibraryFeatures(private val context: Context, private val library: LibraryRepository) {
    private val scope =
        CoroutineScope(
            SupervisorJob() +
                Dispatchers.IO +
                CoroutineExceptionHandler { _, failure ->
                    android.util.Log.e("Resonate", "A background task failed", failure)
                    if (failure is Exception) library.report(failure)
                }
        )
    private val derivedScope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    private val accounts = ProviderAccountStore(context)
    private val settings = context.getSharedPreferences("library-features", Context.MODE_PRIVATE)
    val appearance = AppearanceStore(context.getSharedPreferences("appearance", Context.MODE_PRIVATE))
    val tidalAuthorization = MutableStateFlow<TidalAuthorization?>(null)
    val tidalBusy = MutableStateFlow(false)
    private val artistRequest = AtomicLong()
    private val analysisRequest = AtomicLong()
    val corrections = MutableStateFlow<List<AutoEqDevice>>(emptyList())
    val correctionBusy = MutableStateFlow(false)
    val correctionApplying = MutableStateFlow(false)
    val scrobbleAccount = MutableStateFlow<ScrobbleAccount?>(null)
    val scrobbleStatus = MutableStateFlow<String?>(null)
    private var submitting: Job? = null
    private var missingChecked = -MISSING_CHECKED_EVERY_MS
    private var hidingFailure: String? = null
    private val routes =
        OutputWatch(
            context,
            offered = { device, formats -> library.outputOffersChanged(device, formats) },
            changed = { library.outputRouteChanged(it) },
        )
    val outputStream = MutableStateFlow<OutputStream?>(null)
    val outputs = MutableStateFlow<List<AudioOutput>>(emptyList())
    val selectedOutput = MutableStateFlow(0)
    val actualOutput = MutableStateFlow(0)
    val providerAccounts = MutableStateFlow<ProviderAccountBook?>(null)
    private val downloadSeams =
        object : DownloadSeams {
            override fun accounts() = providerAccounts.value?.asked.orEmpty()

            override fun renewed(token: String) = changeAccounts { it.renewed(token) }

            override fun online() = online.value

            override fun unmetered() = unmetered

            override fun foreground() = foreground

            override fun writable(folder: String?) =
                folder != null &&
                    folders.value.any { it.uri.toString() == folder && it.readable && it.writable }

            override suspend fun landed() {
                library.refresh()
                refreshArtwork()
                enrichment.begin()
                refreshWanted(library.native())
            }
        }
    val downloads = DownloadQueue(context, library, scope, settings, downloadSeams)
    val listening = Listening(library, scope, { online.value }, { auddToken.value })
    val tags = TagEditing(context, library, scope) { songDeletable(it) }
    val downloading: StateFlow<Boolean> =
        downloads.jobs
            .map { jobs -> jobs.any { !it.state.settled } }
            .stateIn(derivedScope, SharingStarted.Eagerly, false)
    val downloadedBytes: StateFlow<ULong> =
        downloads.jobs
            .map(DownloadPlan::received)
            .stateIn(derivedScope, SharingStarted.Eagerly, 0uL)
    val downloadStatus: StateFlow<String?> =
        downloads.jobs
            .map(DownloadPlan::summary)
            .stateIn(derivedScope, SharingStarted.Eagerly, null)
    val online = MutableStateFlow(settings.getBoolean("online", true))
    val hostedServices = MutableStateFlow(settings.getBoolean("hosted-services", true))
    val folders = MutableStateFlow<List<MusicFolder>>(emptyList())
    val organising = FolderOrganising(context, library, settings, scope, ::rereadSongs)
    val statistics = MutableStateFlow<LibraryStatistics?>(null)
    val lyrics = MutableStateFlow<LyricSheet?>(null)
    val lyricFrame = MutableStateFlow<LyricFrame?>(null)
    val lyricsLoading = MutableStateFlow(false)
    val lyricsUnread = MutableStateFlow<ULong?>(null)
    val recognitionKey = MutableStateFlow("")
    val auddToken = MutableStateFlow("")
    val searchAsked = MutableStateFlow<String?>(null)
    val rememberSearches = MutableStateFlow(settings.getBoolean(REMEMBER_SEARCHES_KEY, true))
    val tellsFindings = MutableStateFlow(settings.getBoolean(TELL_FINDINGS_KEY, false))
    val recentSearches = MutableStateFlow(if (rememberSearches.value) readRecentSearches() else emptyList())
    val recognition =
        MutableStateFlow<List<org.resonate.android.nativecore.RecognitionCandidate>>(emptyList())
    val recognitionBusy = MutableStateFlow(false)
    val recognitionTrack = MutableStateFlow<ULong?>(null)
    private val recognitionRequest = AtomicLong()
    private val correctionRequest = AtomicLong()
    val artistAlbumsNext = MutableStateFlow<UInt?>(null)
    val analysis = MutableStateFlow<AnalysisReport?>(null)
    val analysisBusy = MutableStateFlow(false)
    val analysisTrack = MutableStateFlow<ULong?>(null)
    val analysisProgress = MutableStateFlow<Float?>(null)
    val artist = MutableStateFlow<ArtistInformation?>(null)
    val playlists = MutableStateFlow<List<MusicPlaylist>>(emptyList())
    val playlistRows = MutableStateFlow<List<Track>>(emptyList())
    val openPlaylist = MutableStateFlow<ULong?>(null)
    val playlistLoading = MutableStateFlow(false)
    val playlistUnread = MutableStateFlow<ULong?>(null)
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
    private val mutableArtChanges = MutableSharedFlow<List<ULong>>(extraBufferCapacity = 16)
    val artChanges: SharedFlow<List<ULong>> = mutableArtChanges.asSharedFlow()
    val enrichment =
        ArtworkEnrichment(library, settings, scope, { recognitionKey.value }, ::artworkChangedFor)
    val room = RoomCorrectionState(context, library, scope, context.contentResolver)
    private val lyricRequest = AtomicLong()
    private val searchRequest = AtomicLong()
    private var visualization: Job? = null
    val syncConfig = MutableStateFlow<SyncConfig?>(null)
    val syncStatus = MutableStateFlow<SyncStatus?>(null)
    val syncHandoffs = MutableStateFlow<List<HandedOff>>(emptyList())
    val syncFolder = MutableStateFlow(settings.getString("sync-folder", null))
    val syncNotice = MutableStateFlow<String?>(null)
    val pairingOffered = MutableStateFlow<String?>(null)
    @Volatile private var joinWaiting: String? = null
    val playlistSync = MutableStateFlow<PlaylistSync?>(null)
    val transferring = MutableStateFlow(false)
    val transferServiceRunning = MutableStateFlow(false)
    private val syncAccess = Mutex()
    private var syncPolling: Job? = null
    private var network: ConnectivityManager.NetworkCallback? = null
    private val beaconsHeard: WifiManager.MulticastLock? by lazy {
        context.applicationContext
            .getSystemService(WifiManager::class.java)
            ?.createMulticastLock("resonate-sync")
            ?.apply { setReferenceCounted(false) }
    }
    @Volatile private var unmetered = false
    @Volatile private var foreground = true
    private val landingRefused = mutableMapOf<ULong, RefusedLanding>()

    fun downloadServiceReady() {
        downloads.serviceStarted()
    }

    private val pictures =
        object : LruCache<String, Bitmap>(32 * 1024 * 1024) {
            override fun sizeOf(key: String, value: Bitmap) = value.allocationByteCount
        }
    private val encoded =
        object : LruCache<String, ByteArray>(16 * 1024 * 1024) {
            override fun sizeOf(key: String, value: ByteArray) = value.size
        }
    @Volatile private var lyricWatchers = 0

    fun routeInUse(): OutputRoute? = routes.routeNow()

    fun initialize(native: MobilePlayer) {
        room.restore(native)
        routes.start()
        tags.initialize(native)
        providerAccounts.value =
            try {
                accounts.readAccounts()
            } catch (failure: Exception) {
                library.report(failure)
                ProviderAccountBook()
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
        auddToken.value =
            try {
                accounts.readAuddToken()
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
                } catch (cancelled: CancellationException) {
                    throw cancelled
                } catch (failure: Exception) {
                    scrobbleStatus.value = failure.message
                }
                try {
                    lovesSaid(library.native().tellLoves(account))?.let {
                        scrobbleStatus.value = it
                    }
                } catch (cancelled: CancellationException) {
                    throw cancelled
                } catch (failure: Exception) {
                    scrobbleStatus.value = failure.message
                }
            }
        }
        native.configureOnline(online.value)
        native.configureHostedServices(hostedServices.value)
        enrichment.initialize(online.value)
        downloads.recover(native)
        settings.getString("unfinished-sync-landing", null)?.let { value ->
            try {
                val stranded = DownloadPlan.stranded(listOf(value), native.songUris().toSet())
                MusicDocuments.settleLeftover(context, Uri.parse(value), value in stranded)
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
                SyncConfig(
                    enabled = true,
                    nearbyOnly = false,
                    server = "",
                    key = "",
                    device = "",
                    favourites = false,
                    playlists = false,
                    everything = false,
                    meteredAllowed = false,
                )
            }
        joinWaiting?.let(::joinSync)
        watchNetwork()
        syncPolling?.cancel()
        syncPolling = scope.launch { pollSync() }
        work { applySync(it) }
    }

    fun setOnline(enabled: Boolean) {
        work { native ->
            native.configureOnline(enabled)
            online.value = enabled
            downloads.wantsChanged()
            enrichment.onlineChanged(enabled)
            settings.edit().putBoolean("online", enabled).commit()
            applySync(native)
        }
    }

    fun setHostedServices(enabled: Boolean) {
        work { native ->
            native.configureHostedServices(enabled)
            hostedServices.value = enabled
            settings.edit().putBoolean("hosted-services", enabled).commit()
        }
    }

    fun saveSync(config: SyncConfig) {
        syncConfig.value = config
        work { native ->
            accounts.writeSync(config)
            applySync(native)
        }
    }

    fun joinSync(code: String) {
        val config = syncConfig.value
        if (config == null) {
            joinWaiting = code
            library.notice("Joining as soon as this phone's settings are read")
            return
        }
        joinWaiting = null
        saveSync(config.copy(key = code.trim(), enabled = true))
    }

    fun offerPairing(link: String) {
        val code = syncCodeInLink(link)
        when {
            code == null -> library.notice("That link holds no pairing code")
            code == syncStatus.value?.code || code == syncConfig.value?.key ->
                library.notice("This phone already syncs with those devices")
            else -> pairingOffered.value = code
        }
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

    private fun readRecentSearches(): List<String> =
        try {
            val stored = JSONArray(settings.getString(RECENT_SEARCHES_KEY, null) ?: "[]")
            List(stored.length()) { stored.getString(it) }
        } catch (failure: JSONException) {
            emptyList()
        }

    fun setTellsFindings(enabled: Boolean) {
        tellsFindings.value = enabled
        settings.edit().putBoolean(TELL_FINDINGS_KEY, enabled).apply()
    }

    private fun tellFinding(message: String) {
        FindingNotices.shown(tellsFindings.value, message)?.let(library::notice)
    }

    fun setRememberSearches(enabled: Boolean) {
        rememberSearches.value = enabled
        settings.edit().putBoolean(REMEMBER_SEARCHES_KEY, enabled).apply()
        if (!enabled) keepRecentSearches(emptyList())
    }

    fun keepRecentSearches(values: List<String>) {
        val kept = if (rememberSearches.value) values.take(RECENT_SEARCHES_KEPT) else emptyList()
        settings.edit().putString(RECENT_SEARCHES_KEY, JSONArray(kept).toString()).apply()
        recentSearches.value = kept
    }

    fun foreground(visible: Boolean) {
        foreground = visible
        enrichment.foreground(visible)
        downloads.wantsChanged()
        if (visible) forgetMissing(afterScan = false)
    }

    fun loadPlaylistSync(id: ULong) = work { native ->
        playlistSync.value = PlaylistSync(id, native.syncMissingFrom(id), native.syncKeepsSongs(id))
    }

    fun fetchMissing(id: ULong, device: SyncedDevice) = work { native ->
        val asked = native.syncFetchMissing(id, device.id)
        library.notice(
            if (asked > 0u)
                "Asking ${device.name} for $asked ${if (asked == 1u) "song" else "songs"}"
            else "${device.name} has nothing this playlist lacks"
        )
    }

    fun keepSongs(id: ULong, keep: Boolean) = work { native ->
        native.syncKeepSongs(id, keep)
        playlistSync.value = PlaylistSync(id, native.syncMissingFrom(id), native.syncKeepsSongs(id))
    }

    fun continueFrom(handoff: HandedOff, play: () -> Unit) = work { native ->
        val loaded = native.syncContinueFrom(handoff.id)
        library.notice(
            "Continuing ${if (loaded == 1u) "1 song" else "$loaded songs"} from ${handoff.device}"
        )
        if (handoff.playing) withContext(Dispatchers.Main) { play() }
    }

    private fun deviceCalled(): String {
        val named =
            Settings.Global.getString(context.contentResolver, DEVICE_NAME_SETTING)
                ?.takeIf { it.isNotBlank() }
        return syncDeviceCalled(named ?: Build.MODEL.orEmpty())
    }

    private suspend fun applySync(native: MobilePlayer) {
        syncAccess.withLock { syncWith(native) }
    }

    private fun syncWith(native: MobilePlayer) {
        val config = syncConfig.value ?: return
        try {
            val status =
                if (config.enabled && (online.value || config.nearbyOnly))
                    native.syncStart(config.copy(device = config.device.ifBlank { deviceCalled() }))
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
            hearBeacons(status.standing != SyncStanding.OFF)
            syncStatus.value = status
        } catch (failure: Exception) {
            syncStatus.value = native.syncStatus()
            throw failure
        }
    }

    private fun hearBeacons(hear: Boolean) {
        val lock = beaconsHeard ?: return
        if (hear && !lock.isHeld) lock.acquire()
        if (!hear && lock.isHeld) lock.release()
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
        enrichment.networkChanged(free)
        downloads.wantsChanged()
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

    private suspend fun landReceived(native: MobilePlayer, waiting: List<ReceivedSong>) {
        val known = folders.value
        val landing =
            known.map {
                LandingFolder(
                    it.uri.toString(),
                    it.readable,
                    it.writable,
                    it.uri == MusicDocuments.downloads,
                )
            }
        val folder =
            when (val destination = SyncLanding.destination(landing, syncFolder.value)) {
                is LandingDestination.Into -> known.first { it.uri.toString() == destination.id }
                LandingDestination.Away -> {
                    syncNotice.value = SyncLanding.AWAY
                    return
                }
                LandingDestination.Nowhere -> {
                    syncNotice.value = SyncLanding.NOWHERE
                    return
                }
            }
        val now = System.currentTimeMillis()
        landingRefused.keys.retainAll(waiting.map { it.id }.toSet())
        val due = waiting.filter { (landingRefused[it.id]?.until ?: 0L) <= now }
        var landed = 0
        for (received in due) {
            currentCoroutineContext().ensureActive()
            var created: Uri? = null
            try {
                val standing = MusicDocuments.standing(context, folder.uri, received.name)
                if (
                    standing.isNotEmpty() &&
                        native.syncReceiveStanding(received.id, standing) != null
                ) {
                    landingRefused.remove(received.id)
                    landed++
                    continue
                }
                val uri =
                    MusicDocuments.create(
                        context,
                        folder.uri,
                        mimeOf(received.extension),
                        received.name,
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
                MusicDocuments.publish(context, uri)
                settings.edit().remove("unfinished-sync-landing").commit()
                landingRefused.remove(received.id)
                landed++
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (failure: Exception) {
                refuseLanding(native, received.id)
                syncNotice.value = failure.message ?: "A song from another device could not be kept"
                library.report(failure)
            } finally {
                created?.let { uri ->
                    try {
                        MusicDocuments.delete(context, uri)
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
            enrichment.begin()
            tellFinding(FindingNotices.kept(landed))
        }
    }

    private fun refuseLanding(native: MobilePlayer, id: ULong) {
        val tries = (landingRefused[id]?.tries ?: 0) + 1
        if (tries < LANDING_TRIES_AT_MOST) {
            landingRefused[id] =
                RefusedLanding(System.currentTimeMillis() + LANDING_PAUSE_MS, tries)
            return
        }
        landingRefused.remove(id)
        try {
            native.syncRefuseReceived(id)
        } catch (failure: Exception) {
            android.util.Log.w("Resonate", "A song from another device stays waiting", failure)
        }
    }

    suspend fun useDownloads(): MusicFolder =
        withContext(Dispatchers.IO) {
            val uri = MusicDocuments.downloads
            check(
                settings
                    .edit()
                    .putStringSet(
                        "folders",
                        settings.getStringSet("folders", emptySet()).orEmpty() + uri.toString(),
                    )
                    .commit()
            ) {
                "The Downloads choice could not be saved"
            }
            refreshFolders()
            MusicFolder(uri, "Downloads", true, true)
        }

    fun useDownloadsAndScan() {
        scope.launch {
            useDownloads()
            library.scan(MusicDocuments.downloads)
        }
    }

    fun downloadsEnabled() =
        MusicDocuments.downloads.toString() in
            settings.getStringSet("folders", emptySet()).orEmpty()

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
                        if (uri == MusicDocuments.downloads) {
                            return@map MusicFolder(uri, "Downloads", true, true)
                        }
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
                            } catch (failure: RuntimeException) {
                                android.util.Log.w(
                                    "Resonate",
                                    "A music folder could not be read",
                                    failure,
                                )
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

    private fun readableFolders(): List<Uri> {
        val granted =
            context.contentResolver.persistedUriPermissions
                .filter { it.isReadPermission }
                .map { it.uri }
        val trees = granted.filter { treeAnswers(it) }
        val downloads = if (downloadsEnabled()) listOf(MusicDocuments.downloads) else emptyList()
        return trees + downloads
    }

    private fun treeAnswers(tree: Uri): Boolean =
        try {
            val root =
                DocumentsContract.buildDocumentUriUsingTree(
                    tree,
                    DocumentsContract.getTreeDocumentId(tree),
                )
            context.contentResolver
                .query(
                    root,
                    arrayOf(DocumentsContract.Document.COLUMN_DOCUMENT_ID),
                    null,
                    null,
                    null,
                )
                ?.use { it.moveToFirst() } ?: false
        } catch (failure: Exception) {
            false
        }

    fun forgetMissing(afterScan: Boolean) {
        val now = SystemClock.elapsedRealtime()
        if (!afterScan && now - missingChecked < MISSING_CHECKED_EVERY_MS) return
        if (!afterScan && (library.scanning.value || organising.busy.value)) return
        missingChecked = now
        scope.launch {
            try {
                val native = library.native()
                val folders = readableFolders()
                if (folders.isEmpty()) return@launch
                val prefixes = folders.map { MusicDocuments.prefix(it) }
                val listed = folders.flatMap { MusicDocuments.listed(context, it) }
                val changed = native.hideUnlistedSongs(prefixes, listed)
                hidingFailure = null
                afterHiding(native, changed)
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (failure: Exception) {
                if (failure.message != hidingFailure) library.report(failure)
                hidingFailure = failure.message
            }
        }
    }

    private suspend fun afterHiding(native: MobilePlayer, changed: MissingSongs) {
        if (changed.hidden == 0u && changed.restored == 0u) return
        library.refresh()
        openPlaylist.value?.let { id ->
            if (playlists.value.any { it.id == id }) reloadPlaylist(native, id)
        }
        if (changed.hidden > 0u) {
            val fetching =
                syncConfig.value?.let { it.enabled && (it.everything || it.favourites || it.playlists) }
                    ?: false
            tellFinding(FindingNotices.hidden(changed.hidden, fetching))
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
                if (folder.uri != MusicDocuments.downloads) {
                    val native = library.native()
                    afterHiding(native, native.hideSongsUnder(MusicDocuments.prefix(folder.uri)))
                }
            } catch (failure: Exception) {
                library.report(failure)
            }
        }
    }

    fun refreshArtwork() {
        artworkGeneration.value++
    }

    fun artworkChangedFor(songs: List<ULong>) {
        mutableArtChanges.tryEmit(songs)
    }

    suspend fun artworkBitmap(track: Track, edge: Int): Bitmap? = artworkBitmap(track.id, edge)

    private class SeenPicture(val picture: ULong, val generation: Int, val stamp: Int)

    private val seenPictures = java.util.concurrent.ConcurrentHashMap<ULong, SeenPicture>()

    fun cachedArtwork(song: ULong, edge: Int, generation: Int, stamp: Int): Bitmap? {
        val seen = seenPictures[song] ?: return null
        if (seen.generation != generation || seen.stamp != stamp) return null
        return pictures.get("${seen.picture}:$edge")
    }

    suspend fun artworkBitmap(song: ULong, edge: Int, generation: Int = 0, stamp: Int = 0): Bitmap? =
        withContext(Dispatchers.IO) {
            try {
                val native = library.native()
                val picture = native.artworkPicture(song) ?: return@withContext null
                seenPictures[song] = SeenPicture(picture, generation, stamp)
                bitmapOf(native, picture, edge)
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (failure: Exception) {
                android.util.Log.w("Resonate", "A cover could not be drawn", failure)
                null
            }
        }

    suspend fun artworkJpeg(track: Track, edge: Int): ByteArray? =
        withContext(Dispatchers.IO) {
            try {
                val native = library.native()
                val picture = native.artworkPicture(track.id) ?: return@withContext null
                val key = "$picture:$edge"
                encoded.get(key)?.let {
                    return@withContext it
                }
                val bitmap = bitmapOf(native, picture, edge) ?: return@withContext null
                val jpeg =
                    java.io.ByteArrayOutputStream().use { output ->
                        bitmap.compress(Bitmap.CompressFormat.JPEG, 85, output)
                        output.toByteArray()
                    }
                encoded.put(key, jpeg)
                jpeg
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (failure: Exception) {
                android.util.Log.w("Resonate", "A cover could not be encoded", failure)
                null
            }
        }

    private fun bitmapOf(native: MobilePlayer, picture: ULong, edge: Int): Bitmap? {
        val key = "$picture:$edge"
        pictures.get(key)?.let {
            return it
        }
        val bytes = native.picture(picture) ?: return null
        val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        BitmapFactory.decodeByteArray(bytes, 0, bytes.size, bounds)
        if (
            bounds.outWidth <= 0 ||
                bounds.outHeight <= 0 ||
                bounds.outWidth > 32768 ||
                bounds.outHeight > 32768
        )
            return null
        val options =
            BitmapFactory.Options().apply {
                inSampleSize = ArtworkSizes.sampleFor(bounds.outWidth, bounds.outHeight, edge)
            }
        return BitmapFactory.decodeByteArray(bytes, 0, bytes.size, options)?.also {
            pictures.put(key, it)
        }
    }

    suspend fun mediaArtwork(track: Track): ByteArray? =
        withContext(Dispatchers.IO) {
            val source = artworkBitmap(track, ArtworkSizes.MEDIA_EDGE) ?: return@withContext null
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

    fun songDeletable(track: Track): Boolean =
        track.uri.startsWith(OpenedNames.LOCAL_PREFIX) ||
            folders.value.any { it.writable && track.uri.startsWith(MusicDocuments.prefix(it.uri)) }

    fun songsLeft() = work { native -> rereadSongs(native) }

    private suspend fun rereadSongs(native: MobilePlayer) {
        library.refresh()
        playlists.value = native.playlists()
        openPlaylist.value?.let { id ->
            if (playlists.value.any { it.id == id }) reloadPlaylist(native, id)
        }
    }

    fun deleteSongs(tracks: List<Track>, consent: suspend (List<Uri>) -> Boolean) {
        scope.launch {
            try {
                val deleted = SongDeletion(context, library, consent).delete(tracks)
                if (deleted.removed > 0) songsLeft()
                tellDeleted(deleted)
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (failure: Exception) {
                library.report(failure)
            }
        }
    }

    suspend fun deleteCopies(sets: List<IdenticalSongs>, consent: suspend (List<Uri>) -> Boolean) {
        val deleted = SongDeletion(context, library, consent).foldCopies(sets)
        if (deleted.folded > 0) songsLeft()
        if (deleted.kept > 0) library.report(IllegalStateException(DeletionSentences.kept(deleted.kept)))
        else if (deleted.folded > 0) library.notice(DeletionSentences.folded(deleted.folded))
    }

    private fun tellDeleted(deleted: Deleted) {
        if (deleted.kept > 0) library.report(IllegalStateException(DeletionSentences.kept(deleted.kept)))
        else library.notice(DeletionSentences.deleted(deleted.titles))
    }

    fun loadPlaylists() = work { native -> playlists.value = native.playlists() }

    fun selectPlaylist(id: ULong?) {
        openPlaylist.value = id
        playlistRows.value = emptyList()
        playlistLoading.value = id != null
        playlistUnread.value = null
        if (id != null)
            work { native ->
                var read = false
                try {
                    reloadPlaylist(native, id)
                    read = true
                } finally {
                    if (openPlaylist.value == id) {
                        if (!read) playlistUnread.value = id
                        playlistLoading.value = false
                    }
                }
            }
    }

    fun createPlaylist(name: String, tracks: List<Track> = emptyList()) = work { native ->
        val id = native.createPlaylist(name)
        if (tracks.isNotEmpty()) native.addToPlaylist(id, tracks.map { it.id })
        playlists.value = native.playlists()
        if (openPlaylist.value == id) reloadPlaylist(native, id)
    }

    fun saveSearch(name: String, search: SavedSearch) = work { native ->
        native.saveSearch(name, search)
        playlists.value = native.playlists()
        library.notice("Saved “$name” among your playlists")
    }

    fun addToPlaylist(id: ULong, track: Track) = addToPlaylist(id, listOf(track))

    fun addToPlaylist(id: ULong, tracks: List<Track>) = work { native ->
        native.addToPlaylist(id, tracks.map { it.id })
        playlists.value = native.playlists()
        if (openPlaylist.value == id) reloadPlaylist(native, id)
    }

    fun editPlaylist(action: (MobilePlayer) -> Unit) = work { native ->
        action(native)
        playlists.value = native.playlists()
        openPlaylist.value?.let { id ->
            if (playlists.value.any { it.id == id }) reloadPlaylist(native, id)
            else selectPlaylist(null)
        }
    }

    private fun reloadPlaylist(native: MobilePlayer, id: ULong) {
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
        if (lyrics.value?.trackId != id) {
            lyrics.value = null
            lyricFrame.value = null
        }
        lyricsUnread.value = null
        lyricsLoading.value = true
        work { native ->
            var read = false
            try {
                val sheet = native.lyrics(id, refresh)
                read = true
                if (request == lyricRequest.get()) lyrics.value = sheet
            } finally {
                if (request == lyricRequest.get()) {
                    if (!read) lyricsUnread.value = id
                    lyricsLoading.value = false
                }
            }
        }
    }

    @Synchronized
    fun watchLyrics(watching: Boolean) {
        lyricWatchers = if (watching) lyricWatchers + 1 else (lyricWatchers - 1).coerceAtLeast(0)
    }

    @Synchronized
    fun stopWatchingLyrics() {
        lyricWatchers = 0
    }

    suspend fun followLyrics(native: MobilePlayer, place: StateFlow<Place?>) {
        var failing: String? = null
        while (currentCoroutineContext().isActive) {
            delay(LYRIC_FRAME_EVERY_MS)
            if (lyricWatchers == 0 || !foreground) continue
            try {
                val sheet = lyrics.value ?: continue
                val at = place.value?.takeIf { it.track == sheet.trackId } ?: continue
                val position = at.now().toULong()
                lyricFrame.value =
                    native.keptLyricFrame(sheet.trackId, position)
                        ?: native.lyricFrame(sheet.trackId, position)
                failing = null
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (failure: Exception) {
                if (failure.message != failing) library.report(failure)
                failing = failure.message
            }
        }
    }

    fun tick(native: MobilePlayer, snapshot: Snapshot) {
        enrichment.playing(snapshot.current?.id)
        val device = native.outputDevice()
        actualOutput.value = device
        outputStream.value = native.outputStream()
        routes.streamOn(device)
        if (downloading.value) downloads.follow(native)
        if (analysisBusy.value) analysisProgress.value = native.analysisProgress()
    }

    fun saveAuddToken(value: String) = work {
        val token = value.trim()
        accounts.writeAuddToken(token)
        auddToken.value = token
        library.notice(
            if (token.isEmpty()) "AudD will no longer be asked"
            else "AudD will be asked when Shazam does not know a song"
        )
    }

    fun saveRecognitionKey(value: String) = work {
        accounts.writeRecognitionKey(value)
        recognitionKey.value = value
        enrichment.begin()
    }

    fun recognise(id: ULong) {
        val request = recognitionRequest.incrementAndGet()
        recognition.value = emptyList()
        recognitionTrack.value = id
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
        analysisProgress.value = null
        analysisTrack.value = id
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
        val request = correctionRequest.incrementAndGet()
        correctionBusy.value = true
        work { native ->
            try {
                val found = native.searchAutoEq(text)
                if (request == correctionRequest.get()) corrections.value = found
            } finally {
                if (request == correctionRequest.get()) correctionBusy.value = false
            }
        }
    }

    fun clearCorrections() {
        correctionRequest.incrementAndGet()
        corrections.value = emptyList()
        correctionBusy.value = false
    }

    fun applyCorrection(id: String, name: String) {
        correctionApplying.value = true
        work { native ->
            try {
                val found = native.autoEqProfile(id)
                if (found == null) {
                    library.notice("AutoEq could not give a correction for $name")
                    return@work
                }
                library.updatePreferences {
                    it.copy(equaliserEnabled = true, equaliser = found.curve)
                }
                library.notice("Applied ${found.curve.name}")
            } finally {
                correctionApplying.value = false
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

    fun validateScrobbling(account: ScrobbleAccount) = work { native ->
        scrobbleStatus.value = native.validateScrobbling(account).joinToString(" · ")
    }

    fun signInLastFm(account: ScrobbleAccount, user: String, password: String, onSignedIn: () -> Unit) = work { native ->
        val session = native.lastfmSignIn(account.lastfmKey, account.lastfmSecret, user, password)
        val kept = scrobbleAccount.value ?: account.copy(listenbrainzToken = "")
        val signedIn =
            kept.copy(
                lastfmKey = account.lastfmKey,
                lastfmSecret = account.lastfmSecret,
                lastfmSession = session,
            )
        accounts.writeScrobbling(signedIn)
        native.startScrobbling(ScrobbleService.LAST_FM, false)
        scrobbleAccount.value = signedIn
        scrobbleStatus.value = "Signed in to Last.fm"
        onSignedIn()
    }

    fun signOutLastFm() = work { _ ->
        val kept = scrobbleAccount.value ?: return@work
        val signedOut = kept.copy(lastfmSession = "")
        accounts.writeScrobbling(signedOut)
        scrobbleAccount.value = signedOut
        scrobbleStatus.value = "Signed out of Last.fm"
    }

    fun refreshOutputs() {
        scope.launch {
            val manager = context.getSystemService(android.media.AudioManager::class.java)
            outputs.value = audioOutputs(manager)
        }
    }

    fun selectOutput(id: Int) {
        library.request { native ->
            native.selectOutput(id)
            selectedOutput.value = id
        }
    }

    fun authorizeTidal(id: Long, account: DownloadAccount) {
        tidalBusy.value = true
        work { native ->
            try {
                tidalAuthorization.value =
                    native.authorizeTidal(account.clientId, account.clientSecret)
                val token = native.finishTidalAuthorization() ?: return@work
                changeAccounts { it.saved(id, account.copy(refreshToken = token)) }
                tidalAuthorization.value = null
                downloads.wantsChanged()
                library.notice("Signed in to TIDAL")
            } finally {
                tidalBusy.value = false
            }
        }
    }

    private fun changeAccounts(edit: (ProviderAccountBook) -> ProviderAccountBook) {
        synchronized(accounts) { providerAccounts.value = accounts.change(edit) }
    }

    fun keepProviderAccount(id: Long, account: DownloadAccount): Long {
        var kept = id
        changeAccounts { book ->
            if (id == ProviderAccountBook.UNSAVED) kept = book.nextId()
            book.saved(kept, account)
        }
        return kept
    }

    fun saveProviderAccount(id: Long, account: DownloadAccount) = work {
        keepProviderAccount(id, account)
        downloads.wantsChanged()
        library.notice("Download source saved")
    }

    fun turnProviderAccount(id: Long, on: Boolean) = work {
        changeAccounts { it.turned(id, on) }
        downloads.wantsChanged()
    }

    fun removeProviderAccount(id: Long) = work {
        changeAccounts { it.removed(id) }
        downloads.wantsChanged()
        library.notice("Download source removed")
    }

    fun download(song: SearchSong, folder: MusicFolder) {
        download(listOf(song), folder)
    }

    fun download(songs: List<SearchSong>, folder: MusicFolder) {
        if (providerAccounts.value?.ready != true) {
            library.notice("Set up a download provider first")
            return
        }
        downloads.enqueue(songs, folder.uri)
    }

    fun cancelDownload() = downloads.cancelAll()

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

    private fun refreshWanted(native: MobilePlayer) {
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

    private fun <T> onlineWork(action: (MobilePlayer) -> T, publish: (T) -> Unit) {
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
        routes.stop()
        enrichment.close()
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
        hearBeacons(false)
        unmetered = false
        transferring.value = false
        syncHandoffs.value = emptyList()
        analysisRequest.incrementAndGet()
        recognitionRequest.incrementAndGet()
        correctionRequest.incrementAndGet()
        recognitionBusy.value = false
        artistRequest.incrementAndGet()
        lyricRequest.incrementAndGet()
        searchRequest.incrementAndGet()
        analysisBusy.value = false
        lyricsLoading.value = false
        lyricsUnread.value = null
        searching.value = false
        enriching.value = false
        tidalBusy.value = false
        correctionBusy.value = false
        correctionApplying.value = false
    }

    private fun work(action: suspend (MobilePlayer) -> Unit) {
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
