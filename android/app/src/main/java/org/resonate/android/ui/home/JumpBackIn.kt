package org.resonate.android.ui.home

import java.time.LocalDate
import java.time.ZonedDateTime
import org.resonate.android.R
import org.resonate.android.Words
import org.resonate.android.nativecore.Track
import org.resonate.android.nativecore.TrackFilter
import org.resonate.android.nativecore.TrackOrder
import org.resonate.android.nativecore.albumKey
import org.resonate.android.ui.albumOwner
import org.resonate.android.ui.navigation.Screen

sealed interface JumpBackTile {
    data class Album(val key: String, val title: String, val artist: String, val cover: ULong) : JumpBackTile

    data object Favourites : JumpBackTile
}

val JumpBackTile.id: String
    get() =
        when (this) {
            is JumpBackTile.Album -> "album:$key"
            JumpBackTile.Favourites -> "favourites"
        }

val JumpBackTile.screen: Screen
    get() =
        when (this) {
            is JumpBackTile.Album -> Screen.Album(key, title, artist, cover)
            JumpBackTile.Favourites -> Screen.Favourites
        }

fun JumpBackTile.title(words: Words): String =
    when (this) {
        is JumpBackTile.Album -> title
        JumpBackTile.Favourites -> words.string(R.string.library_favourites)
    }

fun JumpBackTile.spoken(words: Words): String =
    when (this) {
        is JumpBackTile.Album ->
            if (artist.isBlank()) title else words.string(R.string.home_jump_back_in_spoken, title, artist)
        JumpBackTile.Favourites -> title(words)
    }

object JumpBackIn {
    const val SONGS_READ = 100u
    const val ALBUMS_KEPT = 8
    const val FEWEST_SHOWN = 2

    fun heardLately(base: TrackFilter): TrackFilter =
        base.copy(played = true, order = TrackOrder.PLAYED, descending = true)

    fun albums(
        heard: List<Track>,
        keeping: Int = ALBUMS_KEPT,
        keyOf: (Track) -> String? = { albumKey(it.album, it.artist, it.albumArtist) },
    ): List<JumpBackTile.Album> {
        val seen = mutableSetOf<String>()
        val tiles = mutableListOf<JumpBackTile.Album>()
        for (song in heard) {
            if (tiles.size >= keeping) break
            if (song.album.isEmpty()) continue
            val key = keyOf(song) ?: continue
            if (!seen.add(key)) continue
            tiles += JumpBackTile.Album(key, song.album, song.albumOwner, song.id)
        }
        return tiles
    }

    fun shown(albums: List<JumpBackTile.Album>, favourites: Boolean, columns: Int): List<JumpBackTile> {
        val places = JumpBackGrid.tiles(columns)
        val tiles = if (favourites) albums.take(places - 1) + JumpBackTile.Favourites else albums.take(places)
        return if (tiles.size < FEWEST_SHOWN) emptyList() else tiles
    }
}

object JumpBackGrid {
    fun columns(wide: Boolean, largeText: Boolean): Int =
        when {
            largeText -> 1
            wide -> 4
            else -> 2
        }

    fun tiles(columns: Int): Int =
        when {
            columns <= 1 -> 4
            columns <= 3 -> 6
            else -> 8
        }
}

data class HomeMoment(val catalogRevision: ULong, val day: LocalDate) {
    companion object {
        fun of(catalogRevision: ULong, now: ZonedDateTime = ZonedDateTime.now()): HomeMoment =
            HomeMoment(catalogRevision, now.toLocalDate())
    }
}
