package org.resonate.android

import androidx.media3.common.C
import androidx.media3.common.Player
import org.junit.Assert.assertEquals
import org.junit.Test

class SeekingTest {
    private val first = 10
    private val playing = 13

    @Test
    fun previousFromAnySessionReachesTheEnginesPreviousNeverARowJump() {
        val restarting = Seeking.of(Player.COMMAND_SEEK_TO_PREVIOUS, 3, 0, first, playing)
        val stepping = Seeking.of(Player.COMMAND_SEEK_TO_PREVIOUS, 2, C.TIME_UNSET, first, playing)
        val itemOnly =
            Seeking.of(Player.COMMAND_SEEK_TO_PREVIOUS_MEDIA_ITEM, 2, C.TIME_UNSET, first, playing)

        assertEquals(Seeking.Previous, restarting)
        assertEquals(Seeking.Previous, stepping)
        assertEquals(Seeking.Previous, itemOnly)
    }

    @Test
    fun nextFromAnySessionReachesTheEnginesNextNeverARowJump() {
        val next = Seeking.of(Player.COMMAND_SEEK_TO_NEXT, 4, C.TIME_UNSET, first, playing)
        val itemOnly =
            Seeking.of(Player.COMMAND_SEEK_TO_NEXT_MEDIA_ITEM, 4, C.TIME_UNSET, first, playing)
        val repeatingTheOnlyRow =
            Seeking.of(Player.COMMAND_SEEK_TO_NEXT, 3, C.TIME_UNSET, first, playing)

        assertEquals(Seeking.Next, next)
        assertEquals(Seeking.Next, itemOnly)
        assertEquals(Seeking.Next, repeatingTheOnlyRow)
    }

    @Test
    fun aChosenRowStillJumpsToItAtTheAskedPlace() {
        val chosen = Seeking.of(Player.COMMAND_SEEK_TO_MEDIA_ITEM, 6, 2_500, first, playing)
        val atItsStart =
            Seeking.of(Player.COMMAND_SEEK_TO_MEDIA_ITEM, 0, C.TIME_UNSET, first, playing)

        assertEquals(Seeking.ToRow(16u, 2_500u), chosen)
        assertEquals(Seeking.ToRow(10u, 0u), atItsStart)
    }

    @Test
    fun aSeekWithinTheSongPlayingMovesOnlyItsPlace() {
        val within =
            Seeking.of(Player.COMMAND_SEEK_IN_CURRENT_MEDIA_ITEM, 3, 42_000, first, playing)
        val theRowPlaying = Seeking.of(Player.COMMAND_SEEK_TO_MEDIA_ITEM, 3, 7_000, first, playing)
        val beforeTheStart =
            Seeking.of(Player.COMMAND_SEEK_IN_CURRENT_MEDIA_ITEM, 3, -5, first, playing)

        assertEquals(Seeking.Within(42_000u), within)
        assertEquals(Seeking.Within(7_000u), theRowPlaying)
        assertEquals(Seeking.Within(0u), beforeTheStart)
    }
}
