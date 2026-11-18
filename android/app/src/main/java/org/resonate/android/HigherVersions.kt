package org.resonate.android

import org.resonate.android.nativecore.HigherVersion
import org.resonate.android.nativecore.HigherVersionPicked

enum class HigherStanding {
    FOUND,
    CHOSEN,
    WAITING,
    NONE,
}

data class HigherKept(val chosen: Int, val downloading: Int, val passedOver: Int)

object HigherVersions {
    private const val PARTS_APART = "; "

    fun standing(song: HigherVersion): HigherStanding =
        when {
            song.pending -> HigherStanding.WAITING
            song.found == null -> HigherStanding.NONE
            song.chosen -> HigherStanding.CHOSEN
            else -> HigherStanding.FOUND
        }

    fun tickable(song: HigherVersion): Boolean = standing(song) == HigherStanding.FOUND

    fun ticked(songs: List<HigherVersion>): Set<ULong> =
        songs.filter(::tickable).map { it.song.id }.toSet()

    fun stillTicked(songs: List<HigherVersion>, ticked: Set<ULong>): Set<ULong> =
        ticked.intersect(ticked(songs))

    fun picked(songs: List<HigherVersion>, ticked: Set<ULong>): List<HigherVersionPicked> =
        songs.mapNotNull { song ->
            val found = song.found ?: return@mapNotNull null
            if (song.song.id in ticked && tickable(song)) HigherVersionPicked(song.song.id, found.version)
            else null
        }

    fun line(words: Words, song: HigherVersion): String =
        when (standing(song)) {
            HigherStanding.FOUND ->
                words.string(R.string.higher_found, song.song.codec, song.found?.label.orEmpty())
            HigherStanding.CHOSEN ->
                words.string(R.string.higher_chosen, song.song.codec, song.found?.label.orEmpty())
            HigherStanding.WAITING -> words.string(R.string.higher_waiting, song.song.codec)
            HigherStanding.NONE -> words.string(R.string.higher_none, song.song.codec)
        }

    fun told(words: Words, done: HigherKept): String =
        listOfNotNull(
            done.chosen.takeIf { it > 0 }?.let { words.plural(R.plurals.higher_told_chosen, it, it) },
            done.downloading.takeIf { it > 0 }?.let { words.plural(R.plurals.higher_told_downloading, it, it) },
            done.passedOver.takeIf { it > 0 }?.let { words.plural(R.plurals.higher_told_passed_over, it, it) },
        )
            .ifEmpty { listOf(words.string(R.string.higher_told_nothing)) }
            .joinToString(PARTS_APART)
}
