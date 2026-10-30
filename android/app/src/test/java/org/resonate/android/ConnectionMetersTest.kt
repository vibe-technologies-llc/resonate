package org.resonate.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import org.resonate.android.nativecore.ConnectionMeter

class ConnectionMetersTest {
    @Test
    fun aNetworkWithTheNotMeteredCapabilityIsUnmeteredAndOneWithoutItIsMetered() {
        assertEquals(ConnectionMeter.UNMETERED, ConnectionMeters.of(true))
        assertEquals(ConnectionMeter.METERED, ConnectionMeters.of(false))
    }

    @Test
    fun aLostNetworkIsUnknownWhichIsNotMetered() {
        assertEquals(ConnectionMeter.UNKNOWN, ConnectionMeters.of(null))
    }

    @Test
    fun theWaitingLineShowsOnlyWhileDownloadsWaitOnTheMeter() {
        assertEquals("Waiting for a network that is not metered", DownloadPlan.meterLine(true))
        assertNull(DownloadPlan.meterLine(false))
    }
}
