package org.resonate.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import org.resonate.android.nativecore.FavouritesTold

class LovesTest {
    @Test
    fun nothingToldSaysNothing() {
        assertNull(lovesSaid(FavouritesTold(0u, 0u, 0u)))
    }

    @Test
    fun whatWasToldIsOneSentenceWithTheNumbersCounted() {
        assertEquals("Loved 1 favourite", lovesSaid(FavouritesTold(1u, 0u, 0u)))
        assertEquals(
            "Loved 2 favourites, took back 1 love, 3 loves refused",
            lovesSaid(FavouritesTold(2u, 1u, 3u)),
        )
        assertEquals("1 love refused", lovesSaid(FavouritesTold(0u, 0u, 1u)))
    }
}
