package org.resonate.android

import org.resonate.android.nativecore.LossySong

enum class LossyStanding {
    HELD,
    WANTABLE,
    WAITING,
    UNMATCHED,
}

data class LossyReplaced(val replaced: Int, val wanted: Int, val unmatched: Int, val kept: Int)

object LossySongs {
    private const val PARTS_APART = "; "

    fun standing(song: LossySong): LossyStanding =
        when {
            song.lossless != null -> LossyStanding.HELD
            song.pending -> LossyStanding.WAITING
            song.wantable -> LossyStanding.WANTABLE
            else -> LossyStanding.UNMATCHED
        }

    fun tickable(song: LossySong): Boolean =
        when (standing(song)) {
            LossyStanding.HELD,
            LossyStanding.WANTABLE -> true
            LossyStanding.WAITING,
            LossyStanding.UNMATCHED -> false
        }

    fun ticked(songs: List<LossySong>): Set<ULong> =
        songs.filter(::tickable).map { it.song.id }.toSet()

    fun stillTicked(songs: List<LossySong>, ticked: Set<ULong>): Set<ULong> =
        ticked.intersect(songs.filter(::tickable).map { it.song.id }.toSet())

    fun line(words: Words, song: LossySong): String =
        when (standing(song)) {
            LossyStanding.HELD ->
                words.string(R.string.lossy_held, song.song.codec, song.lossless?.codec.orEmpty())
            LossyStanding.WANTABLE -> words.string(R.string.lossy_wantable, song.song.codec)
            LossyStanding.WAITING -> words.string(R.string.lossy_waiting, song.song.codec)
            LossyStanding.UNMATCHED -> words.string(R.string.lossy_unmatched, song.song.codec)
        }

    fun deletes(songs: List<LossySong>, ticked: Set<ULong>): Int =
        songs.count { it.song.id in ticked && standing(it) == LossyStanding.HELD }

    fun told(words: Words, done: LossyReplaced): String =
        listOfNotNull(
            done.replaced.takeIf { it > 0 }?.let { words.plural(R.plurals.lossy_told_replaced, it, it) },
            done.wanted.takeIf { it > 0 }?.let { words.plural(R.plurals.lossy_told_wanted, it, it) },
            done.unmatched.takeIf { it > 0 }?.let { words.plural(R.plurals.lossy_told_unmatched, it, it) },
            done.kept.takeIf { it > 0 }?.let { DeletionSentences.kept(words, it) },
        )
            .ifEmpty { listOf(words.string(R.string.lossy_told_nothing)) }
            .joinToString(PARTS_APART)
}
