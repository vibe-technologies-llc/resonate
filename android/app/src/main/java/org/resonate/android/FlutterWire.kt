package org.resonate.android

import org.resonate.android.nativecore.*

internal fun flutterValue(value: Any?): Any? =
    when (value) {
        null -> null
        is Unit -> null
        is String,
        is Boolean,
        is Int,
        is Long,
        is Double,
        is Float,
        is ByteArray -> value
        is ULong -> value.toString()
        is UInt -> value.toLong()
        is UShort -> value.toInt()
        is UByte -> value.toInt()
        is Enum<*> -> value.name
        is android.net.Uri -> value.toString()
        is List<*> -> value.map(::flutterValue)
        is Map<*, *> -> value.entries.associate { it.key.toString() to flutterValue(it.value) }
        is LyricReads.At -> mapOf("kind" to "at", "line" to value.line.toLong())
        is LyricReads.Spent -> mapOf("kind" to "spent", "line" to value.line?.toLong())
        is LyricReads.Evenly -> mapOf("kind" to "evenly")
        is AlbumHit ->
            mapOf(
                "key" to flutterValue(value.`key`),
                "title" to flutterValue(value.`title`),
                "artist" to flutterValue(value.`artist`),
                "year" to flutterValue(value.`year`),
                "songs" to flutterValue(value.`songs`),
                "durationMs" to flutterValue(value.`durationMs`),
                "artworkTrack" to flutterValue(value.`artworkTrack`),
            )
        is AnalysisReport ->
            mapOf(
                "trackId" to flutterValue(value.`trackId`),
                "verdict" to flutterValue(value.`verdict`),
                "peak" to flutterValue(value.`peak`),
                "rms" to flutterValue(value.`rms`),
                "truePeak" to flutterValue(value.`truePeak`),
                "loudnessLufs" to flutterValue(value.`loudnessLufs`),
                "dynamicRange" to flutterValue(value.`dynamicRange`),
                "clipped" to flutterValue(value.`clipped`),
                "bitsInUse" to flutterValue(value.`bitsInUse`),
                "waveform" to flutterValue(value.`waveform`),
                "spectrum" to flutterValue(value.`spectrum`),
                "spectrumStepHz" to flutterValue(value.`spectrumStepHz`),
                "findings" to flutterValue(value.`findings`),
            )
        is ArtistHit ->
            mapOf(
                "name" to flutterValue(value.`name`),
                "songs" to flutterValue(value.`songs`),
                "albums" to flutterValue(value.`albums`),
                "artworkTrack" to flutterValue(value.`artworkTrack`),
            )
        is ArtistInformation ->
            mapOf(
                "name" to flutterValue(value.`name`),
                "kind" to flutterValue(value.`kind`),
                "country" to flutterValue(value.`country`),
                "area" to flutterValue(value.`area`),
                "genres" to flutterValue(value.`genres`),
                "links" to flutterValue(value.`links`),
                "picture" to flutterValue(value.`picture`),
            )
        is AutoEqDevice ->
            mapOf(
                "id" to flutterValue(value.`id`),
                "name" to flutterValue(value.`name`),
            )
        is ChangedArt ->
            mapOf(
                "revision" to flutterValue(value.`revision`),
                "tracks" to flutterValue(value.`tracks`),
            )
        is DownloadAccount ->
            mapOf(
                "provider" to flutterValue(value.`provider`),
                "server" to flutterValue(value.`server`),
                "user" to flutterValue(value.`user`),
                "password" to flutterValue(value.`password`),
                "clientId" to flutterValue(value.`clientId`),
                "clientSecret" to flutterValue(value.`clientSecret`),
                "refreshToken" to flutterValue(value.`refreshToken`),
            )
        is DownloadOffer ->
            mapOf(
                "job" to flutterValue(value.`job`),
                "extension" to flutterValue(value.`extension`),
                "name" to flutterValue(value.`name`),
                "provider" to flutterValue(value.`provider`),
                "total" to flutterValue(value.`total`),
            )
        is DownloadProgress ->
            mapOf(
                "phase" to flutterValue(value.`phase`),
                "received" to flutterValue(value.`received`),
                "total" to flutterValue(value.`total`),
                "provider" to flutterValue(value.`provider`),
            )
        is EnrichmentOptions ->
            mapOf(
                "refresh" to flutterValue(value.`refresh`),
                "atMost" to flutterValue(value.`atMost`),
            )
        is EnrichmentStatus ->
            mapOf(
                "running" to flutterValue(value.`running`),
                "due" to flutterValue(value.`due`),
                "asked" to flutterValue(value.`asked`),
                "matched" to flutterValue(value.`matched`),
                "covers" to flutterValue(value.`covers`),
                "portraits" to flutterValue(value.`portraits`),
                "lyrics" to flutterValue(value.`lyrics`),
                "refused" to flutterValue(value.`refused`),
                "stopped" to flutterValue(value.`stopped`),
            )
        is EqualiserBand ->
            mapOf(
                "filter" to flutterValue(value.`filter`),
                "frequencyHz" to flutterValue(value.`frequencyHz`),
                "gainDb" to flutterValue(value.`gainDb`),
                "q" to flutterValue(value.`q`),
            )
        is EqualiserCurve ->
            mapOf(
                "name" to flutterValue(value.`name`),
                "preampDb" to flutterValue(value.`preampDb`),
                "bands" to flutterValue(value.`bands`),
            )
        is EqualiserImport ->
            mapOf(
                "curve" to flutterValue(value.`curve`),
                "passedOver" to flutterValue(value.`passedOver`),
                "approximated" to flutterValue(value.`approximated`),
                "clamped" to flutterValue(value.`clamped`),
                "converted" to flutterValue(value.`converted`),
            )
        is EqualiserLimits ->
            mapOf(
                "bandsAtMost" to flutterValue(value.`bandsAtMost`),
                "lowestHz" to flutterValue(value.`lowestHz`),
                "highestHz" to flutterValue(value.`highestHz`),
                "gainAtMostDb" to flutterValue(value.`gainAtMostDb`),
                "preampAtMostDb" to flutterValue(value.`preampAtMostDb`),
                "qAtLeast" to flutterValue(value.`qAtLeast`),
                "qAtMost" to flutterValue(value.`qAtMost`),
            )
        is HandedOff ->
            mapOf(
                "id" to flutterValue(value.`id`),
                "device" to flutterValue(value.`device`),
                "songs" to flutterValue(value.`songs`),
                "playing" to flutterValue(value.`playing`),
            )
        is ImportedPlaylist ->
            mapOf(
                "id" to flutterValue(value.`id`),
                "matched" to flutterValue(value.`matched`),
                "missing" to flutterValue(value.`missing`),
            )
        is Inspector ->
            mapOf(
                "track" to flutterValue(value.`track`),
                "container" to flutterValue(value.`container`),
                "sampleFormat" to flutterValue(value.`sampleFormat`),
                "seekable" to flutterValue(value.`seekable`),
                "encoder" to flutterValue(value.`encoder`),
                "copyright" to flutterValue(value.`copyright`),
                "comment" to flutterValue(value.`comment`),
                "isrc" to flutterValue(value.`isrc`),
            )
        is LibrarySearch ->
            mapOf(
                "tracks" to flutterValue(value.`tracks`),
                "trackCount" to flutterValue(value.`trackCount`),
                "albums" to flutterValue(value.`albums`),
                "artists" to flutterValue(value.`artists`),
                "reads" to flutterValue(value.`reads`),
                "didYouMean" to flutterValue(value.`didYouMean`),
                "completions" to flutterValue(value.`completions`),
            )
        is LibraryStatistics ->
            mapOf(
                "songs" to flutterValue(value.`songs`),
                "albums" to flutterValue(value.`albums`),
                "artists" to flutterValue(value.`artists`),
                "durationMs" to flutterValue(value.`durationMs`),
                "plays" to flutterValue(value.`plays`),
                "listenedMs" to flutterValue(value.`listenedMs`),
                "topTracks" to flutterValue(value.`topTracks`),
            )
        is LyricFrame ->
            mapOf(
                "line" to flutterValue(value.`line`),
                "active" to flutterValue(value.`active`),
                "voiceOne" to flutterValue(value.`voiceOne`),
                "voiceTwo" to flutterValue(value.`voiceTwo`),
                "readAt" to flutterValue(value.`readAt`),
                "reads" to flutterValue(value.`reads`),
                "waiting" to flutterValue(value.`waiting`),
                "ended" to flutterValue(value.`ended`),
                "wipes" to flutterValue(value.`wipes`),
            )
        is LyricLetter ->
            mapOf(
                "startUtf16" to flutterValue(value.`startUtf16`),
                "endUtf16" to flutterValue(value.`endUtf16`),
                "share" to flutterValue(value.`share`),
            )
        is LyricRow ->
            mapOf(
                "text" to flutterValue(value.`text`),
                "atMs" to flutterValue(value.`atMs`),
                "untilMs" to flutterValue(value.`untilMs`),
                "voice" to flutterValue(value.`voice`),
                "words" to flutterValue(value.`words`),
                "blank" to flutterValue(value.`blank`),
                "written" to flutterValue(value.`written`),
                "breathes" to flutterValue(value.`breathes`),
            )
        is LyricSheet ->
            mapOf(
                "trackId" to flutterValue(value.`trackId`),
                "origin" to flutterValue(value.`origin`),
                "detail" to flutterValue(value.`detail`),
                "twoVoices" to flutterValue(value.`twoVoices`),
                "end" to flutterValue(value.`end`),
                "writtenEnd" to flutterValue(value.`writtenEnd`),
                "lines" to flutterValue(value.`lines`),
            )
        is LyricWait ->
            mapOf(
                "next" to flutterValue(value.`next`),
                "through" to flutterValue(value.`through`),
            )
        is LyricWipe ->
            mapOf(
                "line" to flutterValue(value.`line`),
                "sungUtf16" to flutterValue(value.`sungUtf16`),
                "blend" to flutterValue(value.`blend`),
            )
        is LyricWord ->
            mapOf(
                "text" to flutterValue(value.`text`),
                "atMs" to flutterValue(value.`atMs`),
                "untilMs" to flutterValue(value.`untilMs`),
            )
        is MissingSongs ->
            mapOf(
                "hidden" to flutterValue(value.`hidden`),
                "restored" to flutterValue(value.`restored`),
                "leftTheQueue" to flutterValue(value.`leftTheQueue`),
            )
        is MixAccents ->
            mapOf(
                "from" to flutterValue(value.`from`),
                "to" to flutterValue(value.`to`),
            )
        is MusicPlaylist ->
            mapOf(
                "id" to flutterValue(value.`id`),
                "name" to flutterValue(value.`name`),
                "songs" to flutterValue(value.`songs`),
                "revision" to flutterValue(value.`revision`),
                "covers" to flutterValue(value.`covers`),
            )
        is OnlineCollection ->
            mapOf(
                "name" to flutterValue(value.`name`),
                "songs" to flutterValue(value.`songs`),
                "releases" to flutterValue(value.`releases`),
                "artists" to flutterValue(value.`artists`),
            )
        is PlaybackPreferences ->
            mapOf(
                "equaliserEnabled" to flutterValue(value.`equaliserEnabled`),
                "equaliser" to flutterValue(value.`equaliser`),
                "replayGain" to flutterValue(value.`replayGain`),
                "quality" to flutterValue(value.`quality`),
                "phase" to flutterValue(value.`phase`),
                "truePeak" to flutterValue(value.`truePeak`),
                "restoration" to flutterValue(value.`restoration`),
                "bufferMs" to flutterValue(value.`bufferMs`),
                "dsdLikePcm" to flutterValue(value.`dsdLikePcm`),
            )
        is ReceivedSong ->
            mapOf(
                "id" to flutterValue(value.`id`),
                "name" to flutterValue(value.`name`),
                "extension" to flutterValue(value.`extension`),
                "song" to flutterValue(value.`song`),
            )
        is RecognitionCandidate ->
            mapOf(
                "score" to flutterValue(value.`score`),
                "song" to flutterValue(value.`song`),
            )
        is ScrobbleAccount ->
            mapOf(
                "listenbrainzToken" to flutterValue(value.`listenbrainzToken`),
                "lastfmKey" to flutterValue(value.`lastfmKey`),
                "lastfmSecret" to flutterValue(value.`lastfmSecret`),
                "lastfmSession" to flutterValue(value.`lastfmSession`),
            )
        is SearchAlbum ->
            mapOf(
                "id" to flutterValue(value.`id`),
                "title" to flutterValue(value.`title`),
                "artist" to flutterValue(value.`artist`),
                "date" to flutterValue(value.`date`),
            )
        is SearchArtist ->
            mapOf(
                "id" to flutterValue(value.`id`),
                "name" to flutterValue(value.`name`),
                "kind" to flutterValue(value.`kind`),
            )
        is SearchDiscography ->
            mapOf(
                "albums" to flutterValue(value.`albums`),
                "next" to flutterValue(value.`next`),
            )
        is SearchRelease ->
            mapOf(
                "id" to flutterValue(value.`id`),
                "title" to flutterValue(value.`title`),
                "date" to flutterValue(value.`date`),
                "country" to flutterValue(value.`country`),
                "songs" to flutterValue(value.`songs`),
            )
        is SearchSong ->
            mapOf(
                "recording" to flutterValue(value.`recording`),
                "title" to flutterValue(value.`title`),
                "artist" to flutterValue(value.`artist`),
                "album" to flutterValue(value.`album`),
                "durationMs" to flutterValue(value.`durationMs`),
                "isrcs" to flutterValue(value.`isrcs`),
                "release" to flutterValue(value.`release`),
                "releaseGroup" to flutterValue(value.`releaseGroup`),
                "artistId" to flutterValue(value.`artistId`),
                "disc" to flutterValue(value.`disc`),
                "position" to flutterValue(value.`position`),
                "held" to flutterValue(value.`held`),
            )
        is Snapshot ->
            mapOf(
                "transport" to flutterValue(value.`transport`),
                "current" to flutterValue(value.`current`),
                "queueIndex" to flutterValue(value.`queueIndex`),
                "positionMs" to flutterValue(value.`positionMs`),
                "durationMs" to flutterValue(value.`durationMs`),
                "volume" to flutterValue(value.`volume`),
                "shuffle" to flutterValue(value.`shuffle`),
                "repeat" to flutterValue(value.`repeat`),
                "outputRate" to flutterValue(value.`outputRate`),
                "underruns" to flutterValue(value.`underruns`),
                "failure" to flutterValue(value.`failure`),
                "sleep" to flutterValue(value.`sleep`),
                "sleepRemainingMs" to flutterValue(value.`sleepRemainingMs`),
                "queueRevision" to flutterValue(value.`queueRevision`),
                "catalogRevision" to flutterValue(value.`catalogRevision`),
            )
        is SyncConfig ->
            mapOf(
                "nearbyOnly" to flutterValue(value.`nearbyOnly`),
                "enabled" to flutterValue(value.`enabled`),
                "server" to flutterValue(value.`server`),
                "key" to flutterValue(value.`key`),
                "device" to flutterValue(value.`device`),
                "favourites" to flutterValue(value.`favourites`),
                "playlists" to flutterValue(value.`playlists`),
                "everything" to flutterValue(value.`everything`),
                "meteredAllowed" to flutterValue(value.`meteredAllowed`),
            )
        is SyncStatus ->
            mapOf(
                "standing" to flutterValue(value.`standing`),
                "code" to flutterValue(value.`code`),
                "devices" to flutterValue(value.`devices`),
                "moving" to flutterValue(value.`moving`),
                "owed" to flutterValue(value.`owed`),
                "arrivals" to flutterValue(value.`arrivals`),
                "settled" to flutterValue(value.`settled`),
                "transfers" to flutterValue(value.`transfers`),
            )
        is SyncTransfer ->
            mapOf(
                "title" to flutterValue(value.`title`),
                "artist" to flutterValue(value.`artist`),
                "from" to flutterValue(value.`from`),
                "received" to flutterValue(value.`received`),
                "size" to flutterValue(value.`size`),
                "state" to flutterValue(value.`state`),
            )
        is SyncedDevice ->
            mapOf(
                "id" to flutterValue(value.`id`),
                "name" to flutterValue(value.`name`),
                "platform" to flutterValue(value.`platform`),
                "online" to flutterValue(value.`online`),
                "nearby" to flutterValue(value.`nearby`),
                "thisDevice" to flutterValue(value.`thisDevice`),
            )
        is TextRange ->
            mapOf(
                "start" to flutterValue(value.`start`),
                "end" to flutterValue(value.`end`),
            )
        is TidalAuthorization ->
            mapOf(
                "userCode" to flutterValue(value.`userCode`),
                "verifyAt" to flutterValue(value.`verifyAt`),
            )
        is Track ->
            mapOf(
                "id" to flutterValue(value.`id`),
                "uri" to flutterValue(value.`uri`),
                "name" to flutterValue(value.`name`),
                "title" to flutterValue(value.`title`),
                "artist" to flutterValue(value.`artist`),
                "album" to flutterValue(value.`album`),
                "durationMs" to flutterValue(value.`durationMs`),
                "rate" to flutterValue(value.`rate`),
                "codec" to flutterValue(value.`codec`),
                "favourite" to flutterValue(value.`favourite`),
                "albumArtist" to flutterValue(value.`albumArtist`),
                "genre" to flutterValue(value.`genre`),
                "date" to flutterValue(value.`date`),
                "trackNumber" to flutterValue(value.`trackNumber`),
                "discNumber" to flutterValue(value.`discNumber`),
                "bits" to flutterValue(value.`bits`),
                "channels" to flutterValue(value.`channels`),
                "recording" to flutterValue(value.`recording`),
                "release" to flutterValue(value.`release`),
                "artistMbid" to flutterValue(value.`artistMbid`),
                "artRevision" to flutterValue(value.`artRevision`),
                "queueId" to flutterValue(value.`queueId`),
            )
        is TrackFilter ->
            mapOf(
                "text" to flutterValue(value.`text`),
                "genre" to flutterValue(value.`genre`),
                "artist" to flutterValue(value.`artist`),
                "album" to flutterValue(value.`album`),
                "minimumRate" to flutterValue(value.`minimumRate`),
                "minimumBits" to flutterValue(value.`minimumBits`),
                "favourites" to flutterValue(value.`favourites`),
                "unplayed" to flutterValue(value.`unplayed`),
                "order" to flutterValue(value.`order`),
                "descending" to flutterValue(value.`descending`),
            )
        is WantedSong ->
            mapOf(
                "id" to flutterValue(value.`id`),
                "song" to flutterValue(value.`song`),
                "held" to flutterValue(value.`held`),
            )
        is MusicFolder ->
            mapOf(
                "uri" to value.uri.toString(),
                "name" to value.name,
                "readable" to value.readable,
                "writable" to value.writable,
            )
        is AudioOutput ->
            mapOf(
                "id" to value.id,
                "name" to value.name,
                "kind" to value.kind,
                "rates" to value.rates,
                "channels" to value.channels,
                "encodings" to value.encodings,
            )
        is PlaylistSync ->
            mapOf(
                "playlist" to value.playlist.toString(),
                "missing" to value.missing.toLong(),
                "kept" to value.kept,
            )
        is SleepTimer.After -> mapOf("kind" to "after", "seconds" to value.seconds.toLong())
        is SleepTimer.EndOfTrack -> mapOf("kind" to "track")
        is SleepTimer.EndOfQueue -> mapOf("kind" to "queue")
        else -> error("Unsupported Flutter value ${value.javaClass.simpleName}")
    }
