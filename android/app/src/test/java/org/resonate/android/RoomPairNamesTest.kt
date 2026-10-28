package org.resonate.android

import org.junit.Assert.assertEquals
import org.junit.Test

class RoomPairNamesTest {
    @Test
    fun aPairIsNamedByBothFilesStemsLeftFirst() {
        assertEquals("Studio L and Studio R", pairedRoomName("Studio L.wav", "Studio R.flac"))
        assertEquals("left.take.2 and right", pairedRoomName("left.take.2.wav", "right"))
    }

    @Test
    fun aFilesStemAndExtensionAreSplitAtItsLastDot() {
        assertEquals("room.v2", roomStem("room.v2.wav"))
        assertEquals("wav", roomExtension("room.v2.wav"))
        assertEquals("room", roomStem("room"))
        assertEquals("", roomExtension("room"))
    }
}
