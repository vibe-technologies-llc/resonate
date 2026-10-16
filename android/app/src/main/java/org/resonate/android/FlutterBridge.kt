package org.resonate.android

import androidx.media3.session.MediaController
import io.flutter.plugin.common.BinaryMessenger
import io.flutter.plugin.common.EventChannel
import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.MethodChannel
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.resonate.android.nativecore.*

private const val PAGE_ROWS = 500u

@androidx.annotation.OptIn(markerClass = [androidx.media3.common.util.UnstableApi::class])
class FlutterBridge(
    private val activity: MainActivity,
    private val library: LibraryRepository,
    messenger: BinaryMessenger,
    private val controller: () -> MediaController?,
) : MethodChannel.MethodCallHandler, EventChannel.StreamHandler {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private val methods = MethodChannel(messenger, "resonate/player")
    private val events = EventChannel(messenger, "resonate/state")
    private var watching: Job? = null
    private val features
        get() = library.features

    init {
        methods.setMethodCallHandler(this)
        events.setStreamHandler(this)
    }

    override fun onListen(arguments: Any?, sink: EventChannel.EventSink) {
        watching?.cancel()
        watching = scope.launch {
            fun watch(name: String, flow: Flow<*>) = launch {
                flow.collect { value ->
                    val encoded = withContext(Dispatchers.Default) { flutterValue(value) }
                    sink.success(mapOf(name to encoded))
                }
            }
            watch("tracks", library.tracks.distinctUntilChanged(::sameSongs))
            watch("allTracks", library.allTracks.distinctUntilChanged(::sameSongs))
            watch(
                "favourite",
                library.favourites.map { mapOf("id" to it.id, "enabled" to it.enabled) },
            )
            watch("queue", library.queue)
            watch("snapshot", library.snapshot)
            watch("status", library.status)
            watch("scanning", library.scanning)
            watch("preferences", library.preferences)
            watch("filter", library.filter)
            watch("online", features.online)
            watch("enrichmentAutomatic", features.enrichment.automatic)
            watch("enrichmentUnmeteredOnly", features.enrichment.unmeteredOnly)
            watch("enrichmentStatus", features.enrichment.status)
            watch("coversAskedAgain", features.enrichment.coversAskedAgain)
            watch("folders", features.folders)
            watch("playlists", features.playlists)
            watch(
                "playlistRows",
                combine(features.playlistRows, features.playlistLoading) { rows, loading ->
                    if (loading) null else rows
                },
            )
            watch("statistics", features.statistics)
            watch("lyrics", features.lyrics)
            watch("lyricFrame", features.lyricFrame)
            watch("lyricsLoading", features.lyricsLoading)
            watch("analysis", features.analysis)
            watch("analysisBusy", features.analysisBusy)
            watch("analysisProgress", features.analysisProgress)
            watch("inspector", features.inspector)
            watch("artist", features.artist)
            watch("samples", features.samples)
            watch("artworkGeneration", features.artworkGeneration)
            watch("artChanged", features.artChanges)
            watch("onlineSongs", features.onlineSongs)
            watch("onlineAlbums", features.onlineAlbums)
            watch("artistAlbumsNext", features.artistAlbumsNext)
            watch("onlineArtists", features.onlineArtists)
            watch("onlineReleases", features.onlineReleases)
            watch("onlineCollection", features.onlineCollection)
            watch("searching", features.searching)
            watch("wanted", features.wanted)
            watch("downloading", features.downloading)
            watch("downloadedBytes", features.downloadedBytes)
            watch("downloadStatus", features.downloadStatus)
            watch("providerAccount", features.providerAccount)
            watch("tidalAuthorization", features.tidalAuthorization)
            watch("tidalBusy", features.tidalBusy)
            watch("scrobbleAccount", features.scrobbleAccount)
            watch("scrobbleStatus", features.scrobbleStatus)
            watch("corrections", features.corrections)
            watch("correctionBusy", features.correctionBusy)
            watch("outputs", features.outputs)
            watch("selectedOutput", features.selectedOutput)
            watch("actualOutput", features.actualOutput)
            watch("syncConfig", features.syncConfig)
            watch("syncFolder", features.syncFolder)
            watch("syncStatus", features.syncStatus)
            watch("syncHandoffs", features.syncHandoffs)
            watch("syncNotice", features.syncNotice)
            watch("playlistSync", features.playlistSync)
            watch(
                "recognition",
                combine(features.recognition, features.recognitionTrack) { candidates, track ->
                    mapOf("trackId" to track, "candidates" to candidates)
                },
            )
            watch("recognitionBusy", features.recognitionBusy)
            watch("recognitionKey", features.recognitionKey)
        }
    }

    override fun onCancel(arguments: Any?) {
        watching?.cancel()
        watching = null
    }

    override fun onMethodCall(call: MethodCall, result: MethodChannel.Result) {
        scope.launch {
            try {
                val answer = dispatch(call)
                result.success(flutterValue(answer))
            } catch (cancelled: CancellationException) {
                result.error("cancelled", "The screen was closed", null)
                throw cancelled
            } catch (failure: kotlin.Exception) {
                result.error("native", failure.message ?: "The action could not be completed", null)
            }
        }
    }

    private suspend fun dispatch(call: MethodCall): Any? {
        val args = call.arguments as? Map<*, *> ?: emptyMap<Any, Any>()
        fun text(key: String) = args[key]?.toString().orEmpty()
        fun number(key: String) = text(key).toLongOrNull() ?: 0L
        fun id(key: String = "id") = text(key).toULong()
        fun flag(key: String) = args[key] == true
        fun ids() = (args["ids"] as List<*>).map { it.toString().toULong() }
        fun track(value: ULong = id()) =
            checkNotNull(library.findTrack(value)) { "That song is no longer in the library" }
        fun tracks() = ids().map { track(it) }
        fun filter(): TrackFilter =
            defaultTrackFilter()
                .copy(
                    favourites = flag("favourites"),
                    unplayed = flag("unplayed"),
                    artist = text("artist"),
                    album = text("album"),
                    genre = text("genre"),
                    minimumRate = number("minimumRate").toUInt(),
                    minimumBits = number("minimumBits").toUInt(),
                    order =
                        TrackOrder.entries.firstOrNull { it.name == text("order") }
                            ?: TrackOrder.TITLE,
                    descending = flag("descending"),
                )
        fun song(): SearchSong =
            features.onlineSongs.value.firstOrNull { it.recording == text("recording") }
                ?: features.wanted.value.first { it.id == id() }.song
        when (call.method) {
            "chooseFolder" -> activity.chooseFolder()
            "useDownloads" -> activity.useDownloads()
            "notifications" -> activity.allowNotifications()
            "search" -> library.search(text("text"))
            "filter" -> {
                val orderName = text("order")
                val isReset =
                    args.isEmpty() ||
                        (!flag("favourites") &&
                            !flag("unplayed") &&
                            text("artist").isEmpty() &&
                            text("album").isEmpty() &&
                            text("genre").isEmpty() &&
                            number("minimumRate") == 0L &&
                            number("minimumBits") == 0L &&
                            (orderName.isEmpty() || orderName == TrackOrder.TITLE.name) &&
                            !flag("descending"))
                if (isReset) library.clearFilter() else library.setFilter(filter())
            }
            "play" -> {
                val media = tracks().map { it.mediaItem() }
                val remote = checkNotNull(controller()) { "Playback is still connecting" }
                remote.setMediaItems(media, number("index").toInt(), 0)
                remote.prepare()
                remote.play()
            }
            "toggle" -> {
                val remote = checkNotNull(controller()) { "Playback is still connecting" }
                if (remote.playWhenReady) remote.pause() else remote.play()
            }
            "previous" -> checkNotNull(controller()).seekToPreviousMediaItem()
            "next" -> checkNotNull(controller()).seekToNextMediaItem()
            "seek" -> checkNotNull(controller()).seekTo(number("position"))
            "jump" -> checkNotNull(controller()).seekToDefaultPosition(number("index").toInt())
            "shuffle" -> checkNotNull(controller()).shuffleModeEnabled = flag("enabled")
            "repeat" -> checkNotNull(controller()).repeatMode = number("mode").toInt()
            "volume" -> checkNotNull(controller()).volume = (args["value"] as Number).toFloat()
            "favourite" -> library.setFavourite(id(), flag("enabled"))
            "playNext" -> {
                val rows = ids()
                library.request { it.playNext(rows) }
            }
            "addToQueue" -> {
                val rows = ids()
                library.request { it.addToQueue(rows) }
            }
            "removeFromQueue" -> {
                val row = id()
                library.request { it.removeFromQueue(row) }
            }
            "moveInQueue" ->
                library.request { it.moveInQueue(number("from").toUInt(), number("to").toUInt()) }
            "clearQueue" -> library.request { it.clearQueue() }
            "rescan" -> library.rescan()
            "dismissStatus" -> library.dismissStatus()
            "online" -> features.setOnline(flag("enabled"))
            "refreshFolders" -> features.refreshFolders()
            "removeFolder" ->
                features.removeFolder(
                    features.folders.value.first { it.uri.toString() == text("uri") }
                )
            "artwork" ->
                return features.artworkJpeg(
                    track(),
                    ArtworkSizes.edgeFor(number("size").toInt()),
                )
            "collections" ->
                return withContext(Dispatchers.IO) {
                    val native = library.native()
                    mapOf(
                        "albums" to flutterValue(native.albums(CollectionOrder.ALPHABETICAL)),
                        "artists" to flutterValue(native.artists(CollectionOrder.ALPHABETICAL)),
                    )
                }
            "albumsBy" ->
                return withContext(Dispatchers.IO) { library.native().albumsBy(text("name")) }
            "collectionTracks" ->
                return withContext(Dispatchers.IO) {
                    val native = library.native()
                    when (text("kind")) {
                        "album" -> native.albumTracks(text("key"))
                        "mix" -> {
                            val mix = filter()
                            allPages { native.filteredTracks(mix, it, PAGE_ROWS) }
                        }
                        else -> native.artistTracks(text("key"))
                    }
                }
            "albumKey" ->
                return albumKey(text("album"), text("artist"), text("albumArtist"))
            "loadPlaylists" -> features.loadPlaylists()
            "selectPlaylist" -> features.selectPlaylist(id())
            "createPlaylist" -> features.createPlaylist(text("name"))
            "addToPlaylist" -> features.addToPlaylist(id(), tracks())
            "renamePlaylist" ->
                features.editPlaylist { it.renamePlaylist(id(), id("revision"), text("name")) }
            "deletePlaylist" -> features.editPlaylist { it.deletePlaylist(id(), id("revision")) }
            "moveInPlaylist" ->
                features.editPlaylist {
                    it.moveInPlaylist(
                        id(),
                        id("revision"),
                        number("from").toUInt(),
                        number("to").toUInt(),
                    )
                }
            "removeFromPlaylist" ->
                features.editPlaylist {
                    it.removeFromPlaylist(id(), id("revision"), number("position").toUInt())
                }
            "undoPlaylist" -> features.editPlaylist { it.undoPlaylist() }
            "statistics" -> features.loadStatistics(number("days").toUInt())
            "inspector" -> features.loadInspector(id())
            "artist" -> features.loadArtist(id())
            "lyrics" -> features.loadLyrics(id(), flag("refresh"))
            "analyse" -> features.analyse(id())
            "cancelAnalysis" -> features.cancelAnalysis()
            "visualize" -> features.visualize(flag("enabled"))
            "lyricFrames" -> features.watchLyrics(flag("enabled"))
            "enrich" -> features.enrich(tracks())
            "enrichmentAutomatic" -> features.enrichment.setAutomatic(flag("enabled"))
            "enrichmentUnmeteredOnly" -> features.enrichment.setUnmeteredOnly(flag("enabled"))
            "enrichmentStatus" -> features.enrichment.read()
            "refreshEnrichment" -> features.enrichment.refresh()
            "retryMissingCovers" -> features.enrichment.retryMissingCovers()
            "stopEnrichment" -> features.enrichment.stop()
            "recognise" -> features.recognise(id())
            "recognitionKey" -> features.saveRecognitionKey(text("value"))
            "searchOnline" -> features.searchOnline(text("text"))
            "searchAlbums" -> features.searchAlbums(text("text"))
            "searchArtists" -> features.searchArtists(text("text"))
            "onlineReleases" -> features.onlineReleases(text("id"))
            "releaseSongs" -> features.releaseSongs(text("id"))
            "artistAlbums" -> features.artistAlbums(text("id"), number("from").toUInt())
            "openLink" -> features.openLink(text("text"))
            "clearCollection" -> features.clearCollection()
            "saveWanted" -> features.saveWanted(listOf(song()))
            "loadWanted" -> features.loadWanted()
            "removeWanted" -> features.removeWanted(id())
            "importPlaylist" ->
                features.importPlaylist(
                    text("name"),
                    features.onlineCollection.value?.songs.orEmpty(),
                )
            "download" ->
                features.download(
                    song(),
                    features.folders.value.first { it.uri.toString() == text("uri") },
                )
            "downloadToDownloads" -> features.download(song(), features.useDownloads())
            "cancelDownload" -> features.cancelDownload()
            "provider" -> features.saveProviderAccount(provider(args))
            "authorizeTidal" -> features.authorizeTidal(provider(args))
            "scrobbling" -> features.saveScrobbling(scrobbling(args), flag("includeHistory"))
            "validateScrobbling" -> features.validateScrobbling(scrobbling(args))
            "lastFm" -> features.signInLastFm(scrobbling(args), text("user"), text("password"))
            "corrections" -> features.searchCorrections(text("text"))
            "applyCorrection" -> features.applyCorrection(text("id"))
            "profile" -> features.applyProfileText(text("name"), text("text"))
            "outputs" -> features.refreshOutputs()
            "output" -> features.selectOutput(number("id").toInt())
            "sleep" ->
                library.request {
                    it.setSleep(
                        when (text("kind")) {
                            "after" -> SleepTimer.After(number("seconds").toUInt())
                            "track" -> SleepTimer.EndOfTrack
                            "queue" -> SleepTimer.EndOfQueue
                            else -> null
                        }
                    )
                }
            "preferences" ->
                library.updatePreferences { current ->
                    current.copy(
                        equaliserEnabled =
                            args["equaliserEnabled"] as? Boolean ?: current.equaliserEnabled,
                        truePeak = args["truePeak"] as? Boolean ?: current.truePeak,
                        dsdLikePcm = args["dsdLikePcm"] as? Boolean ?: current.dsdLikePcm,
                        bufferMs =
                            (args["bufferMs"] as? Number)?.toInt()?.toUInt() ?: current.bufferMs,
                        replayGain =
                            GainMode.entries.firstOrNull { it.name == text("replayGain") }
                                ?: current.replayGain,
                        quality =
                            ResamplingQuality.entries.firstOrNull { it.name == text("quality") }
                                ?: current.quality,
                        phase =
                            ResamplingPhase.entries.firstOrNull { it.name == text("phase") }
                                ?: current.phase,
                        restoration =
                            LossyRestoration.entries.firstOrNull { it.name == text("restoration") }
                                ?: current.restoration,
                    )
                }
            "equaliser" ->
                library.updatePreferences { current ->
                    val bands =
                        (args["bands"] as? List<*>)?.map { item ->
                            val band = item as Map<*, *>
                            val filterName =
                                (band["filter"] as? String)?.uppercase() ?: "PEAKING"
                            val filter =
                                try {
                                    EqualiserFilter.valueOf(filterName)
                                } catch (_: IllegalArgumentException) {
                                    EqualiserFilter.PEAKING
                                }
                            EqualiserBand(
                                filter,
                                (band["frequencyHz"] as Number).toFloat(),
                                (band["gainDb"] as Number).toFloat(),
                                (band["q"] as Number).toFloat(),
                            )
                        }
                    val name = args["name"] as? String ?: current.equaliser.name
                    val preamp =
                        (args["preamp"] as? Number)?.toFloat()
                            ?: current.equaliser.preampDb
                    val gains = (args["gains"] as? List<*>)?.map { (it as Number).toFloat() }
                    current.copy(
                        equaliser =
                            if (flag("reset")) EqualiserCurve("", 0f, emptyList())
                            else if (bands != null) EqualiserCurve(name, preamp, bands)
                            else if (gains != null)
                                tenBandCurve((args["preamp"] as Number).toFloat(), gains)
                            else
                                current.equaliser.copy(
                                    preampDb = (args["preamp"] as Number).toFloat()
                                )
                    )
                }
            "equaliserInfo" ->
                return withContext(Dispatchers.Default) {
                    val curve = library.preferences.value?.equaliser
                    val rate = library.snapshot.value?.outputRate?.takeIf { it != 0u }
                    val response =
                        try {
                            curve?.let { responseCurve(it, 256u, rate) }
                        } catch (_: Exception) {
                            try {
                                curve?.let { responseCurve(it, 256u, null) }
                            } catch (_: Exception) {
                                null
                            }
                        }
                    mapOf(
                        "limits" to flutterValue(equaliserLimits()),
                        "frequencies" to equaliserFrequencies(),
                        "gains" to curve?.let { tenBandGains(it) },
                        "response" to response,
                    )
                }
            "openUrl" -> {
                val uri = android.net.Uri.parse(text("url"))
                require(uri.scheme == "https" || uri.scheme == "http")
                activity.startActivity(
                    android.content.Intent(android.content.Intent.ACTION_VIEW, uri)
                )
            }
            "equaliserGains" ->
                return library.preferences.value?.equaliser?.let { tenBandGains(it) }
            "sync" ->
                features.syncConfig.value?.let { current ->
                    features.saveSync(
                        current.copy(
                            enabled = args["enabled"] as? Boolean ?: current.enabled,
                            nearbyOnly = args["nearbyOnly"] as? Boolean ?: current.nearbyOnly,
                            server = args["server"] as? String ?: current.server,
                            device = args["device"] as? String ?: current.device,
                            favourites = args["favourites"] as? Boolean ?: current.favourites,
                            playlists = args["playlists"] as? Boolean ?: current.playlists,
                            everything = args["everything"] as? Boolean ?: current.everything,
                            meteredAllowed =
                                args["meteredAllowed"] as? Boolean ?: current.meteredAllowed,
                        )
                    )
                }
            "joinSync" -> features.joinSync(text("code"))
            "newSyncGroup" -> features.newSyncGroup()
            "syncFolder" ->
                features.chooseSyncFolder(
                    features.folders.value.first { it.uri.toString() == text("uri") }
                )
            "forceSync" ->
                withContext(Dispatchers.IO) {
                    library.native().syncForce()
                }
            "forgetSyncDevice" ->
                withContext(Dispatchers.IO) {
                    library.native().syncForgetDevice(text("id"))
                }
            "searchLibrary" ->
                return withContext(Dispatchers.IO) {
                    library
                        .native()
                        .searchLibrary(
                            text("text"),
                            number("limit").toUInt().takeIf { it > 0u } ?: 500u,
                        )
                }
            "saveQueueAsPlaylist" ->
                return withContext(Dispatchers.IO) {
                    val native = library.native()
                    val id = native.createPlaylist(text("name"))
                    val rows = library.queue.value.map { it.id }
                    if (rows.isNotEmpty()) {
                        native.addToPlaylist(id, rows)
                    }
                    features.loadPlaylists()
                    id.toString()
                }
            "fetchMissing" ->
                features.fetchMissing(
                    id(),
                    features.syncStatus.value?.devices?.first { it.id == text("device") }
                        ?: throw IllegalArgumentException("The device is no longer available"),
                )
            "keepSongs" -> features.keepSongs(id(), flag("enabled"))
            "playlistSync" -> features.loadPlaylistSync(id())
            "handoff" ->
                features.continueFrom(features.syncHandoffs.value[number("index").toInt()]) {
                    controller()?.play()
                }
            else -> throw IllegalArgumentException("Unknown player action ${call.method}")
        }
        return null
    }

    private fun provider(args: Map<*, *>) =
        DownloadAccount(
            MusicProvider.entries.first { it.name == args["provider"] },
            args["server"] as? String ?: "",
            args["user"] as? String ?: "",
            args["password"] as? String ?: "",
            args["clientId"] as? String ?: "",
            args["clientSecret"] as? String ?: "",
            args["refreshToken"] as? String ?: "",
        )

    private fun scrobbling(args: Map<*, *>) =
        ScrobbleAccount(
            args["listenbrainzToken"] as? String ?: "",
            args["lastfmKey"] as? String ?: "",
            args["lastfmSecret"] as? String ?: "",
            args["lastfmSession"] as? String ?: "",
        )

    private fun <T> allPages(read: (UInt) -> List<T>): List<T> {
        val found = ArrayList<T>()
        do {
            val page = read(found.size.toUInt())
            found.addAll(page)
        } while (page.size == PAGE_ROWS.toInt())
        return found
    }

    fun close() {
        features.visualize(false)
        features.stopWatchingLyrics()
        methods.setMethodCallHandler(null)
        events.setStreamHandler(null)
        scope.cancel()
    }
}
