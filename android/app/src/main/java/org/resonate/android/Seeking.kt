package org.resonate.android

import androidx.media3.common.Player

sealed interface Seeking {
    data object Previous : Seeking

    data object Next : Seeking

    data class Within(val positionMs: ULong) : Seeking

    data class ToRow(val row: UInt, val positionMs: ULong) : Seeking

    companion object {
        fun of(
            seekCommand: Int,
            mediaItemIndex: Int,
            positionMs: Long,
            first: Int,
            playing: Int?,
        ): Seeking {
            val position = positionMs.coerceAtLeast(0).toULong()
            val row = first + mediaItemIndex.coerceAtLeast(0)

            return when (seekCommand) {
                Player.COMMAND_SEEK_TO_PREVIOUS,
                Player.COMMAND_SEEK_TO_PREVIOUS_MEDIA_ITEM -> Previous
                Player.COMMAND_SEEK_TO_NEXT,
                Player.COMMAND_SEEK_TO_NEXT_MEDIA_ITEM -> Next
                Player.COMMAND_SEEK_IN_CURRENT_MEDIA_ITEM -> Within(position)
                else -> if (row == playing) Within(position) else ToRow(row.toUInt(), position)
            }
        }
    }
}
