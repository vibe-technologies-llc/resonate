package org.resonate.android.ui.server

import androidx.compose.runtime.Composable
import org.resonate.android.R
import org.resonate.android.Words
import org.resonate.android.nativecore.ServedAlbum
import org.resonate.android.nativecore.ServedArtist
import org.resonate.android.nativecore.ServedFound
import org.resonate.android.nativecore.ServedSong
import org.resonate.android.ui.clock
import org.resonate.android.ui.length
import org.resonate.android.words

fun ServedArtist.shownName(words: Words): String =
    name.ifBlank { words.string(R.string.unknown_artist) }

val ServedArtist.shownName: String
    @Composable get() = shownName(words())

fun ServedAlbum.shownName(words: Words): String =
    name.ifBlank { words.string(R.string.server_unknown_album) }

val ServedAlbum.shownName: String
    @Composable get() = shownName(words())

fun ServedSong.shownTitle(words: Words): String =
    title.ifBlank { words.string(R.string.unknown_song) }

val ServedSong.shownTitle: String
    @Composable get() = shownTitle(words())

fun artistLine(words: Words, artist: ServedArtist): String? =
    artist.albums?.let { words.plural(R.plurals.count_albums, it.toInt(), it.toInt()) }

fun albumLine(words: Words, album: ServedAlbum): String =
    listOfNotNull(
            album.artist?.takeIf { it.isNotBlank() },
            album.year?.toString(),
            album.songs?.let { words.plural(R.plurals.count_songs, it.toInt(), it.toInt()) },
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

fun albumSummary(words: Words, songs: List<ServedSong>): String =
    listOfNotNull(
            words.plural(R.plurals.count_songs, songs.size, songs.size),
            songs
                .mapNotNull { it.durationMs }
                .takeIf { it.isNotEmpty() }
                ?.let { length(words, it.sum()) },
        )
        .joinToString(" · ")

fun artistSummary(words: Words, albums: List<ServedAlbum>): String =
    words.plural(R.plurals.count_albums, albums.size, albums.size)

private fun serviceOr(words: Words, title: String): String =
    title.ifBlank { words.string(R.string.server_this_service) }

fun searchHeadline(words: Words, title: String): String =
    if (title.isBlank()) words.string(R.string.server_search)
    else words.string(R.string.server_search_named, title)

fun firstPageEmpty(words: Words, title: String): String =
    words.string(R.string.server_first_page_empty, serviceOr(words, title))

fun nothingMatches(words: Words, title: String): String =
    words.string(R.string.server_nothing_matches_detail, serviceOr(words, title))

val nothingFound = ServedFound(emptyList(), emptyList(), emptyList())

fun ServedFound.isEmpty(): Boolean = artists.isEmpty() && albums.isEmpty() && songs.isEmpty()

fun foundSummary(words: Words, found: ServedFound): String =
    listOfNotNull(
            found.artists
                .takeIf { it.isNotEmpty() }
                ?.let { words.plural(R.plurals.count_artists, it.size, it.size) },
            found.albums
                .takeIf { it.isNotEmpty() }
                ?.let { words.plural(R.plurals.count_albums, it.size, it.size) },
            found.songs
                .takeIf { it.isNotEmpty() }
                ?.let { words.plural(R.plurals.count_songs, it.size, it.size) },
        )
        .joinToString(" · ")
