package org.resonate.android.ui.server

import org.resonate.android.nativecore.ServedAlbum
import org.resonate.android.nativecore.ServedArtist
import org.resonate.android.nativecore.ServedFound
import org.resonate.android.nativecore.ServedSong
import org.resonate.android.ui.clock
import org.resonate.android.ui.counted
import org.resonate.android.ui.length

val ServedArtist.shownName: String
    get() = name.ifBlank { "Unknown artist" }

val ServedAlbum.shownName: String
    get() = name.ifBlank { "Unknown album" }

val ServedSong.shownTitle: String
    get() = title.ifBlank { "Unknown song" }

fun artistLine(artist: ServedArtist): String? = artist.albums?.let { counted(it, "album") }

fun albumLine(album: ServedAlbum): String =
    listOfNotNull(
            album.artist?.takeIf { it.isNotBlank() },
            album.year?.toString(),
            album.songs?.let { counted(it, "song") },
        )
        .joinToString(" · ")

fun songLine(song: ServedSong, withAlbum: Boolean): String =
    listOfNotNull(
            song.artist?.takeIf { it.isNotBlank() },
            song.album?.takeIf { withAlbum && it.isNotBlank() },
            song.durationMs?.let { clock(it) },
        )
        .joinToString(" · ")

fun songNumber(song: ServedSong, index: Int): String =
    (song.track ?: (index + 1).toUInt()).toString()

fun albumSummary(songs: List<ServedSong>): String =
    listOfNotNull(
            counted(songs.size, "song"),
            songs
                .mapNotNull { it.durationMs }
                .takeIf { it.isNotEmpty() }
                ?.let { length(it.sum()) },
        )
        .joinToString(" · ")

fun artistSummary(albums: List<ServedAlbum>): String = counted(albums.size, "album")

private fun serviceOr(title: String, otherwise: String): String = title.ifBlank { otherwise }

fun searchHeadline(title: String): String = if (title.isBlank()) "Search" else "Search $title"

fun firstPageEmpty(title: String): String =
    "${serviceOr(title, "This service")} shows nothing until you search it. Type words above and press search."

fun nothingMatches(title: String): String =
    "${serviceOr(title, "This service")} found no artist, album or song for those words."

val nothingFound = ServedFound(emptyList(), emptyList(), emptyList())

fun ServedFound.isEmpty(): Boolean = artists.isEmpty() && albums.isEmpty() && songs.isEmpty()

fun foundSummary(found: ServedFound): String =
    listOfNotNull(
            found.artists.takeIf { it.isNotEmpty() }?.let { counted(it.size, "artist") },
            found.albums.takeIf { it.isNotEmpty() }?.let { counted(it.size, "album") },
            found.songs.takeIf { it.isNotEmpty() }?.let { counted(it.size, "song") },
        )
        .joinToString(" · ")
