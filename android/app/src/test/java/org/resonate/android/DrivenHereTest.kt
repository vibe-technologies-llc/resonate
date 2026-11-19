package org.resonate.android

import org.junit.Assert.assertEquals
import org.junit.Test
import org.resonate.android.nativecore.SyncControl

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
}
