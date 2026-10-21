package org.resonate.android

import android.content.Context
import androidx.media3.common.MediaItem
import androidx.media3.common.MediaMetadata
import androidx.media3.common.util.UnstableApi
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
}

@UnstableApi
class LibraryBrowse(private val context: Context, private val library: LibraryRepository) {
    suspend fun root(): MediaItem = folder(BrowseIds.ROOT, context.getString(R.string.browse_root), FOLDER)

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
        val (songs, at) = surroundings(parent, track) ?: return null
        return MediaSession.MediaItemsWithStartPosition(
            songs.map { it.mediaItem() },
            at,
            startPositionMs.coerceAtLeast(0L),
        )
    }

    private suspend fun <T> reading(read: (MobilePlayer) -> T): T =
        withContext(Dispatchers.IO) { read(library.native()) }

    private fun titled(node: BrowseNode): MediaItem? =
        when (node) {
            BrowseNode.Root -> folder(BrowseIds.ROOT, context.getString(R.string.browse_root), FOLDER)
            BrowseNode.Albums ->
                folder(BrowseIds.ALBUMS, context.getString(R.string.browse_albums), FOLDER)
            BrowseNode.Artists ->
                folder(BrowseIds.ARTISTS, context.getString(R.string.browse_artists), FOLDER)
            BrowseNode.Playlists ->
                folder(BrowseIds.PLAYLISTS, context.getString(R.string.browse_playlists), FOLDER)
            BrowseNode.Favourites ->
                folder(BrowseIds.FAVOURITES, context.getString(R.string.browse_favourites), FOLDER)
            else -> null
        }

    private fun listing(native: MobilePlayer, node: BrowseNode): List<MediaItem> =
        when (node) {
            BrowseNode.Root ->
                listOf(BrowseNode.Albums, BrowseNode.Artists, BrowseNode.Playlists, BrowseNode.Favourites)
                    .mapNotNull(::titled)
            BrowseNode.Albums ->
                native.albums(CollectionOrder.ALPHABETICAL).map {
                    folder(BrowseIds.album(it.key), it.title, MediaMetadata.MEDIA_TYPE_ALBUM, it.artist)
                }
            BrowseNode.Artists ->
                native.artists(CollectionOrder.ALPHABETICAL).map {
                    folder(BrowseIds.artist(it.name), it.name, MediaMetadata.MEDIA_TYPE_ARTIST)
                }
            BrowseNode.Playlists ->
                native.playlists().map {
                    folder(BrowseIds.playlist(it.id), it.name, MediaMetadata.MEDIA_TYPE_PLAYLIST)
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

    private fun folder(id: String, title: String, type: Int, subtitle: String? = null): MediaItem =
        MediaItem.Builder()
            .setMediaId(id)
            .setMediaMetadata(
                MediaMetadata.Builder()
                    .setTitle(title)
                    .setSubtitle(subtitle)
                    .setIsBrowsable(true)
                    .setIsPlayable(false)
                    .setMediaType(type)
                    .build()
            )
            .build()

    private fun song(track: Track, parent: String): MediaItem =
        track
            .mediaItem()
            .buildUpon()
            .setMediaId(BrowseIds.song(track.id, parent))
            .build()

    private companion object {
        const val FOLDER = MediaMetadata.MEDIA_TYPE_FOLDER_MIXED
        const val SONGS_AT_MOST = 500u
        const val SEARCH_AT_MOST = 100u
    }
}
