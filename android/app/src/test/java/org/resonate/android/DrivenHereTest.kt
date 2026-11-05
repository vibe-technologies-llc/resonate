package org.resonate.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.resonate.android.nativecore.SyncControl
import org.resonate.android.nativecore.Transport

class DrivenHereTest {
    @Test
    fun aSettingNeverStoredOrUnreadableIsEverythingAndEachReadsBackAsSpelt() {
        assertEquals(SyncControl.EVERYTHING, SyncControls.of(null))
        assertEquals(SyncControl.EVERYTHING, SyncControls.of("a later word"))
        for (control in SyncControl.entries) {
            assertEquals(control, SyncControls.of(SyncControls.spelt(control)))
        }
        assertEquals("transport", SyncControls.spelt(SyncControl.TRANSPORT))
    }

    @Test
    fun soundBeginsOnlyWhereTheTransportWasNotAlreadySounding() {
        assertTrue(soundBegan(Transport.PAUSED, Transport.PLAYING))
        assertTrue(soundBegan(Transport.STOPPED, Transport.LOADING))
        assertTrue(soundBegan(null, Transport.PLAYING))
        assertFalse(soundBegan(Transport.LOADING, Transport.PLAYING))
        assertFalse(soundBegan(Transport.PLAYING, Transport.PAUSED))
        assertFalse(soundBegan(Transport.PAUSED, Transport.PAUSED))
    }
}
