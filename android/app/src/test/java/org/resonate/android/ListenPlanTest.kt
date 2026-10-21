package org.resonate.android

import org.junit.Assert.assertEquals
import org.junit.Test
import org.resonate.android.nativecore.HeardAnswer
import org.resonate.android.nativecore.HeardSong

class ListenPlanTest {
    private fun song(key: String) = HeardSong(key, "Artist", "", null, "", "", null, "", "Shazam", key)

    @Test
    fun aNamedSongIsToldAndAStandingOneIsKeptQuietly() {
        val named = HeardAnswer.Named(song("a"))

        assertEquals(ListenNext.Tell(song("a")), ListenPlan.after(named, 1, null))
        assertEquals(ListenNext.Keep, ListenPlan.after(named, null, song("a")))
        assertEquals(ListenNext.Tell(song("a")), ListenPlan.after(named, null, song("b")))
    }

    @Test
    fun aMissOrSilenceIsTriedAgainUpToThreeTimesThenSettles() {
        assertEquals(ListenNext.Again(2), ListenPlan.after(HeardAnswer.Missed, 1, null))
        assertEquals(ListenNext.Again(3), ListenPlan.after(HeardAnswer.Silent, 2, null))
        assertEquals(ListenNext.Settle(ListenStage.Unknown), ListenPlan.after(HeardAnswer.Missed, 3, null))
        assertEquals(ListenNext.Settle(ListenStage.Silent), ListenPlan.after(HeardAnswer.Silent, 3, null))
    }

    @Test
    fun aServiceOutOfReachIsSaidAtOnce() {
        assertEquals(
            ListenNext.Settle(ListenStage.Unreached("Shazam")),
            ListenPlan.after(HeardAnswer.Unreached("Shazam"), 1, null),
        )
    }

    @Test
    fun followingNeverSaysAMissAndNeverRepeatsTheStandingSong() {
        assertEquals(ListenNext.Keep, ListenPlan.after(HeardAnswer.Missed, null, song("a")))
        assertEquals(ListenNext.Keep, ListenPlan.after(HeardAnswer.Silent, null, null))
    }

    @Test
    fun theLastEightHeardKeepTheNewestFirstAndOneNameOnce() {
        var heard = emptyList<HeardSong>()
        (1..10).forEach { heard = ListenPlan.remembered(heard, song("s$it")) }
        heard = ListenPlan.remembered(heard, song("s5"))

        assertEquals(8, heard.size)
        assertEquals("s5", heard.first().key)
        assertEquals(1, heard.count { it.key == "s5" })
    }
}
