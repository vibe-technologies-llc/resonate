package org.resonate.android

import org.resonate.android.nativecore.HeardAnswer
import org.resonate.android.nativecore.HeardSong

sealed interface ListenStage {
    data object Idle : ListenStage

    data class Recording(val nth: Int) : ListenStage

    data object Asking : ListenStage

    data class Found(val song: HeardSong) : ListenStage

    data object Unknown : ListenStage

    data object Silent : ListenStage

    data class Unreached(val service: String) : ListenStage

    data object Offline : ListenStage

    data object Denied : ListenStage

    data class Failed(val text: String) : ListenStage
}

val ListenStage.isListening: Boolean
    get() = this is ListenStage.Recording || this == ListenStage.Asking

val ListenStage.namedOrMissed: Boolean
    get() = this is ListenStage.Found || this == ListenStage.Unknown

enum class ListenFollowing {
    OFF,
    WAITING,
    LISTENING,
}

sealed interface ListenNext {
    data class Tell(val song: HeardSong) : ListenNext

    data class Again(val nth: Int) : ListenNext

    data class Settle(val stage: ListenStage) : ListenNext

    data object Keep : ListenNext
}

object ListenPlan {
    const val TRIES_ON_A_MISS = 3
    const val CLIP_SECONDS = 12
    const val FOLLOWED_EVERY_SECONDS = 20L
    const val HEARD_KEPT = 8

    fun after(answer: HeardAnswer, naming: Int?, standing: HeardSong?): ListenNext =
        when {
            answer is HeardAnswer.Named && naming == null && standing?.key == answer.song.key ->
                ListenNext.Keep
            answer is HeardAnswer.Named -> ListenNext.Tell(answer.song)
            naming == null -> ListenNext.Keep
            answer is HeardAnswer.Unreached ->
                ListenNext.Settle(ListenStage.Unreached(answer.service))
            naming < TRIES_ON_A_MISS -> ListenNext.Again(naming + 1)
            answer is HeardAnswer.Silent -> ListenNext.Settle(ListenStage.Silent)
            else -> ListenNext.Settle(ListenStage.Unknown)
        }

    fun remembered(heard: List<HeardSong>, song: HeardSong): List<HeardSong> =
        (listOf(song) + heard.filter { it.key != song.key }).take(HEARD_KEPT)
}
