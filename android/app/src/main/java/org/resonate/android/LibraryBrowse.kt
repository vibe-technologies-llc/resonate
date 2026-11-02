package org.resonate.android

import android.content.Context
import android.net.Uri
import android.os.Bundle
import androidx.media3.common.MediaItem
import androidx.media3.common.MediaMetadata
import androidx.media3.common.util.UnstableApi
import androidx.media3.session.MediaConstants
import androidx.media3.session.MediaLibraryService.LibraryParams
import androidx.media3.session.MediaSession
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.resonate.android.nativecore.CollectionOrder
import org.resonate.android.nativecore.MobilePlayer
import org.resonate.android.nativecore.Track
import org.resonate.android.nativecore.defaultTrackFilter

sealed interface BrowseNode {
    data object Root : BrowseNode

    data object Albums : BrowseNode

    data object Artists : BrowseNode

    data object Playlists : BrowseNode

    data object Favourites : BrowseNode

    data class Album(val key: String) : BrowseNode

    data class Artist(val name: String) : BrowseNode

    data class Playlist(val id: ULong) : BrowseNode
}

@UnstableApi
data class BrowseStyle(val browsable: Int, val playable: Int) {
    fun extras(): Bundle =
        Bundle().apply {
            putInt(MediaConstants.EXTRAS_KEY_CONTENT_STYLE_BROWSABLE, browsable)
            putInt(MediaConstants.EXTRAS_KEY_CONTENT_STYLE_PLAYABLE, playable)
        }

    companion object {
        const val LIST = MediaConstants.EXTRAS_VALUE_CONTENT_STYLE_LIST_ITEM
        const val GRID = MediaConstants.EXTRAS_VALUE_CONTENT_STYLE_GRID_ITEM

        fun ofChildren(node: BrowseNode): BrowseStyle =
            when (node) {
                BrowseNode.Albums,
                BrowseNode.Artists,
                BrowseNode.Playlists -> BrowseStyle(GRID, LIST)
                else -> BrowseStyle(LIST, LIST)
            }
    }
}

object BrowseIds {
    const val ROOT = "root"
    const val ALBUMS = "albums"
    const val ARTISTS = "artists"
    const val PLAYLISTS = "playlists"
    const val FAVOURITES = "favourites"
    private const val ALBUM = "album:"
    private const val ARTIST = "artist:"
    private const val PLAYLIST = "playlist:"
    private const val IN = '@'

    fun album(key: String) = ALBUM + key

    fun artist(name: String) = ARTIST + name

    fun playlist(id: ULong) = PLAYLIST + id

    fun song(track: ULong, parent: String) = "$track$IN$parent"

    fun read(id: String): BrowseNode? =
        when {
            id == ROOT -> BrowseNode.Root
            id == ALBUMS -> BrowseNode.Albums
            id == ARTISTS -> BrowseNode.Artists
            id == PLAYLISTS -> BrowseNode.Playlists
            id == FAVOURITES -> BrowseNode.Favourites
            id.startsWith(ALBUM) -> BrowseNode.Album(id.removePrefix(ALBUM))
            id.startsWith(ARTIST) -> BrowseNode.Artist(id.removePrefix(ARTIST))
            id.startsWith(PLAYLIST) ->
                id.removePrefix(PLAYLIST).toULongOrNull()?.let(BrowseNode::Playlist)
            else -> null
        }

    fun songOf(id: String): Pair<ULong, String>? {
        val track = id.substringBefore(IN).toULongOrNull() ?: return null
        val parent = id.substringAfter(IN, "").takeIf { it.isNotEmpty() } ?: return null
        return track to parent
    }

    fun trackOf(id: String): ULong? = songOf(id)?.first ?: id.toULongOrNull()

    fun playable(ids: List<String>, start: Int): Pair<List<ULong>, Int> {
        val read = ids.map(::trackOf)
        val kept = read.filterNotNull()

        val before = read.take(start.coerceAtLeast(0)).count { it != null }
        return kept to before.coerceAtMost((kept.size - 1).coerceAtLeast(0))
    }
}

@UnstableApi
class LibraryBrowse(private val context: Context, private val library: LibraryRepository) {
    suspend fun root(): MediaItem = titled(BrowseNode.Root)!!

    fun rootParams(asked: LibraryParams?): LibraryParams =
        LibraryParams.Builder()
            .setRecent(asked?.isRecent == true)
            .setOffline(asked?.isOffline == true)
            .setSuggested(asked?.isSuggested == true)
            .setExtras(
                BrowseStyle.ofChildren(BrowseNode.Root).extras().apply {
                    putBoolean(CONTENT_STYLE_SUPPORTED, true)
                }
            )
            .build()

    suspend fun item(id: String): MediaItem? {
        val asSong = BrowseIds.songOf(id)
        if (asSong != null) return library.findTrack(asSong.first)?.let { song(it, asSong.second) }
        return BrowseIds.read(id)?.let(::titled)
    }

    suspend fun children(parent: String, page: Int, size: Int): List<MediaItem>? {
        val node = BrowseIds.read(parent) ?: return null
        val all = reading { listing(it, node) }
        return all.drop(page * size).take(size)
    }

    suspend fun surroundings(parent: String, track: ULong): Pair<List<Track>, Int>? {
        val node = BrowseIds.read(parent) ?: return null
        val songs = reading { songsOf(it, node) }
        val at = songs.indexOfFirst { it.id == track }
        return if (at < 0) null else songs to at
    }

    suspend fun found(query: String): List<Track> =
        reading { it.searchLibrary(query, SEARCH_AT_MOST).tracks }

    suspend fun search(query: String): List<MediaItem> =
        found(query).map { song(it, BrowseIds.ROOT) }

    suspend fun expanded(
        items: List<MediaItem>,
        startIndex: Int,
        startPositionMs: Long,
    ): MediaSession.MediaItemsWithStartPosition? {
        val first = items.singleOrNull() ?: return null
        val asked = first.requestMetadata.searchQuery
        if (!asked.isNullOrBlank()) {
            val songs = found(asked)
            return if (songs.isEmpty()) null
            else MediaSession.MediaItemsWithStartPosition(songs.map { it.mediaItem() }, 0, 0L)
        }
        val (track, parent) = BrowseIds.songOf(first.mediaId) ?: return null
        val (songs, at) =
            surroundings(parent, track)
                ?: return MediaSession.MediaItemsWithStartPosition(
                    listOf(alone(first, track)),
                    0,
                    startPositionMs.coerceAtLeast(0L),
                )
        return MediaSession.MediaItemsWithStartPosition(
            songs.map { it.mediaItem() },
            at,
            startPositionMs.coerceAtLeast(0L),
        )
    }

    private fun alone(item: MediaItem, track: ULong): MediaItem =
        library.findTrack(track)?.mediaItem()
            ?: item.buildUpon().setMediaId(track.toString()).build()

    private suspend fun <T> reading(read: (MobilePlayer) -> T): T =
        withContext(Dispatchers.IO) { read(library.native()) }

    private fun titled(node: BrowseNode): MediaItem? =
        when (node) {
            BrowseNode.Root -> topFolder(node, BrowseIds.ROOT, R.string.browse_root)
            BrowseNode.Albums -> topFolder(node, BrowseIds.ALBUMS, R.string.browse_albums)
            BrowseNode.Artists -> topFolder(node, BrowseIds.ARTISTS, R.string.browse_artists)
            BrowseNode.Playlists -> topFolder(node, BrowseIds.PLAYLISTS, R.string.browse_playlists)
            BrowseNode.Favourites ->
                topFolder(node, BrowseIds.FAVOURITES, R.string.browse_favourites)
            else -> null
        }

    private fun topFolder(node: BrowseNode, id: String, title: Int): MediaItem =
        folder(id, context.getString(title), FOLDER, style = BrowseStyle.ofChildren(node))

    private fun listing(native: MobilePlayer, node: BrowseNode): List<MediaItem> =
        when (node) {
            BrowseNode.Root ->
                listOf(BrowseNode.Albums, BrowseNode.Artists, BrowseNode.Playlists, BrowseNode.Favourites)
                    .mapNotNull(::titled)
            BrowseNode.Albums ->
                native.albums(CollectionOrder.ALPHABETICAL).map {
                    folder(
                        BrowseIds.album(it.key),
                        it.title,
                        MediaMetadata.MEDIA_TYPE_ALBUM,
                        it.artist,
                        it.artworkTrack,
                    )
                }
            BrowseNode.Artists ->
                native.artists(CollectionOrder.ALPHABETICAL).map {
                    folder(
                        BrowseIds.artist(it.name),
                        it.name,
                        MediaMetadata.MEDIA_TYPE_ARTIST,
                        artwork = it.artworkTrack,
                    )
                }
            BrowseNode.Playlists ->
                native.playlists().map {
                    folder(
                        BrowseIds.playlist(it.id),
                        it.name,
                        MediaMetadata.MEDIA_TYPE_PLAYLIST,
                        artwork = it.covers.firstOrNull(),
                    )
                }
            else -> songsOf(native, node).map { song(it, parentOf(node)) }
        }

    private fun songsOf(native: MobilePlayer, node: BrowseNode): List<Track> =
        when (node) {
            is BrowseNode.Album -> native.albumTracks(node.key)
            is BrowseNode.Artist -> native.artistTracks(node.name)
            is BrowseNode.Playlist -> pagedPlaylist(native, node.id)
            BrowseNode.Favourites ->
                native.filteredTracks(defaultTrackFilter().copy(favourites = true), 0u, SONGS_AT_MOST)
            else -> emptyList()
        }

    private fun pagedPlaylist(native: MobilePlayer, id: ULong): List<Track> =
        native.playlistTracks(id, 0u, SONGS_AT_MOST)

    private fun parentOf(node: BrowseNode): String =
        when (node) {
            is BrowseNode.Album -> BrowseIds.album(node.key)
            is BrowseNode.Artist -> BrowseIds.artist(node.name)
            is BrowseNode.Playlist -> BrowseIds.playlist(node.id)
            BrowseNode.Favourites -> BrowseIds.FAVOURITES
            else -> BrowseIds.ROOT
        }

    private fun folder(
        id: String,
        title: String,
        type: Int,
        subtitle: String? = null,
        artwork: ULong? = null,
        style: BrowseStyle = BrowseStyle(BrowseStyle.LIST, BrowseStyle.LIST),
    ): MediaItem =
        MediaItem.Builder()
            .setMediaId(id)
            .setMediaMetadata(
                MediaMetadata.Builder()
                    .setTitle(title)
                    .setSubtitle(subtitle)
                    .setArtworkUri(artworkUri(artwork))
                    .setIsBrowsable(true)
                    .setIsPlayable(false)
                    .setMediaType(type)
                    .setExtras(style.extras())
                    .build()
            )
            .build()

    private fun song(track: Track, parent: String): MediaItem {
        val item = track.mediaItem()
        return item
            .buildUpon()
            .setMediaId(BrowseIds.song(track.id, parent))
            .setMediaMetadata(
                item.mediaMetadata
                    .buildUpon()
                    .setArtworkUri(artworkUri(track.id.takeUnless { track.served }))
                    .build()
            )
            .build()
    }

    private fun artworkUri(track: ULong?): Uri? =
        track
            ?.takeIf { it != NO_TRACK }
            ?.let { Uri.parse(ArtworkUris.uri(ArtworkUris.authority(context.packageName), it)) }

    private companion object {
        const val FOLDER = MediaMetadata.MEDIA_TYPE_FOLDER_MIXED
        const val SONGS_AT_MOST = 500u
        const val SEARCH_AT_MOST = 100u
        const val NO_TRACK = 0uL
        const val CONTENT_STYLE_SUPPORTED = "android.media.browse.CONTENT_STYLE_SUPPORTED"
    }
}
