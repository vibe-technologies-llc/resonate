package org.resonate.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.resonate.android.nativecore.Transport

class ZonesHeardTest {
    @Test
    fun aZoneSoundingUnseenHoldsTheSessionWhileTheZoneShownIsStoppedOrEmpty() {
        val shownStopped = ZonesHeard.sounds(Transport.STOPPED, Transport.PLAYING)
        val shownEmpty = ZonesHeard.sounds(null, Transport.LOADING)
        val shownPlaying = ZonesHeard.sounds(Transport.PLAYING, null)
        val nothingSounds = ZonesHeard.sounds(Transport.PAUSED, null)

        assertTrue(shownStopped)
        assertTrue(shownEmpty)
        assertTrue(shownPlaying)
        assertFalse(nothingSounds)
    }

    @Test
    fun theSessionSpeaksForTheZoneHeardOnlyWhileTheZoneShownIsSilent() {
        val shownSilent = ZonesHeard.speaksFor(Transport.STOPPED, Transport.PLAYING, true)
        val shownPlaying = ZonesHeard.speaksFor(Transport.PLAYING, Transport.PLAYING, true)
        val heardNamesNoSong = ZonesHeard.speaksFor(Transport.PAUSED, Transport.LOADING, false)
        val nothingElsewhere = ZonesHeard.speaksFor(Transport.PAUSED, null, false)

        assertEquals(SessionSpeaksFor.ELSEWHERE, shownSilent)
        assertEquals(SessionSpeaksFor.SHOWN, shownPlaying)
        assertEquals(SessionSpeaksFor.SHOWN, heardNamesNoSong)
        assertEquals(SessionSpeaksFor.SHOWN, nothingElsewhere)
    }

    @Test
    fun soundBeginsOnlyWhereNoZoneWasAlreadySounding() {
        fun began(before: Transport?, now: Transport?) =
            ZonesHeard.soundBegan(ZonesHeard.sounds(before, null), ZonesHeard.sounds(now, null))

        assertTrue(began(Transport.PAUSED, Transport.PLAYING))
        assertTrue(began(Transport.STOPPED, Transport.LOADING))
        assertTrue(began(null, Transport.PLAYING))
        assertFalse(began(Transport.LOADING, Transport.PLAYING))
        assertFalse(began(Transport.PLAYING, Transport.PAUSED))
        assertFalse(began(Transport.PAUSED, Transport.PAUSED))
    }

    @Test
    fun focusIsGivenUpOnlyWhenTheLastZoneFallsSilentOfItself() {
        val playingHere = ZonesHeard.sounds(Transport.PLAYING, null)
        val showingAnotherWhileTheFirstPlays = ZonesHeard.sounds(null, Transport.PLAYING)
        val everyZoneSilent = ZonesHeard.sounds(Transport.STOPPED, null)

        assertFalse(ZonesHeard.fellSilentOfItself(playingHere, showingAnotherWhileTheFirstPlays, false))
        assertTrue(ZonesHeard.fellSilentOfItself(showingAnotherWhileTheFirstPlays, everyZoneSilent, false))
        assertFalse(ZonesHeard.fellSilentOfItself(playingHere, everyZoneSilent, true))
        assertFalse(ZonesHeard.soundBegan(playingHere, showingAnotherWhileTheFirstPlays))
        assertTrue(ZonesHeard.soundBegan(everyZoneSilent, showingAnotherWhileTheFirstPlays))
    }
}
